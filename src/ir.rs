use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

fn is_zero_u64(v: &u64) -> bool {
    *v == 0
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum Endianness {
    Little,
    Big,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum BitOrder {
    Lsb0,
    Msb0,
}

impl Default for BitOrder {
    fn default() -> Self {
        BitOrder::Lsb0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum Scalar {
    I8,
    U8,
    I16,
    U16,
    I32,
    U32,
    I64,
    U64,
    F32,
    F64,
}

impl Scalar {
    pub fn size(self) -> u64 {
        match self {
            Scalar::I8 | Scalar::U8 => 1,
            Scalar::I16 | Scalar::U16 => 2,
            Scalar::I32 | Scalar::U32 | Scalar::F32 => 4,
            Scalar::I64 | Scalar::U64 | Scalar::F64 => 8,
        }
    }

    pub fn align(self) -> u64 {
        self.size()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Field {
    pub name: String,
    pub ty: Type,
    pub offset: u64,
    pub size: u64,
    pub align: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StructType {
    pub fields: Vec<Field>,
    pub size: u64,
    pub align: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArrayType {
    pub elem: Box<Type>,
    pub count: u64,
    pub size: u64,
    pub align: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SliceType {
    pub elem: Box<Type>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ColumnarType {
    pub capacity: u64,
    pub columns: Vec<Field>,
    pub size: u64,
    pub align: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BitField {
    pub name: String,
    pub width: u64,
    pub bit_offset: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "BitpackTypeRaw", into = "BitpackTypeRaw")]
pub struct BitpackType {
    pub container: Option<Scalar>,
    pub span: u64,
    pub fields: Vec<BitField>,
    pub size: u64,
    pub align: u64,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BitpackTypeRaw {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    container: Option<Scalar>,
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    span: u64,
    fields: Vec<BitField>,
    size: u64,
    align: u64,
}

impl From<BitpackType> for BitpackTypeRaw {
    fn from(b: BitpackType) -> Self {
        let span = match b.container {
            Some(c) if b.span == c.size() => 0,
            _ => b.span,
        };
        BitpackTypeRaw {
            container: b.container,
            span,
            fields: b.fields,
            size: b.size,
            align: b.align,
        }
    }
}

impl TryFrom<BitpackTypeRaw> for BitpackType {
    type Error = String;

    fn try_from(raw: BitpackTypeRaw) -> std::result::Result<Self, String> {
        let span = match raw.container {
            Some(c) => {
                let expected = c.size();
                if raw.span == 0 {
                    expected
                } else if raw.span != expected {
                    return Err(
                        "bitpack span must equal container size (do not mix container and span)"
                            .into(),
                    );
                } else {
                    raw.span
                }
            }
            None => {
                if raw.span == 0 {
                    return Err("span-only bitpack requires a non-zero span".into());
                }
                raw.span
            }
        };
        Ok(BitpackType {
            container: raw.container,
            span,
            fields: raw.fields,
            size: raw.size,
            align: raw.align,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum Type {
    Scalar(Scalar),
    Struct(StructType),
    Array(ArrayType),
    Slice(SliceType),
    Columnar(ColumnarType),
    /// Named bit-pack overlay: L5 unsigned integer cell or L6 span-only bitstream.
    Bitpack(BitpackType),
    /// `u64` byte offset from the region base. `0` is null; never a process pointer.
    Link,
}

impl Type {
    pub fn scalar(scalar: Scalar) -> Self {
        Type::Scalar(scalar)
    }

    pub fn array(elem: Type, count: u64) -> Result<Self> {
        let elem_size = elem.inline_size();
        let elem_align = elem.align();
        let size = elem_size.checked_mul(count).ok_or(Error::Overflow)?;
        Ok(Type::Array(ArrayType {
            elem: Box::new(elem),
            count,
            size,
            align: elem_align,
        }))
    }

    pub fn slice(elem: Type) -> Self {
        Type::Slice(SliceType {
            elem: Box::new(elem),
        })
    }

    pub fn link() -> Self {
        Type::Link
    }

    pub fn inline_size(&self) -> u64 {
        match self {
            Type::Scalar(s) => s.size(),
            Type::Struct(s) => s.size,
            Type::Array(a) => a.size,
            Type::Slice(_) => 16,
            Type::Columnar(c) => c.size,
            Type::Bitpack(b) => b.size,
            Type::Link => 8,
        }
    }

    pub fn align(&self) -> u64 {
        match self {
            Type::Scalar(s) => s.align(),
            Type::Struct(s) => s.align,
            Type::Array(a) => a.align,
            Type::Slice(_) => 8,
            Type::Columnar(c) => c.align,
            Type::Bitpack(b) => b.align,
            Type::Link => 8,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum Param {
    Ptr,
    Len,
    Scalar(Scalar),
    OwnedPtr,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum Ret {
    Void,
    Scalar(Scalar),
    OwnedPtr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum Lang {
    #[default]
    C,
    Python,
    Llvm,
    Haskell,
    Lean,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Import {
    pub symbol: String,
    pub args: Vec<Param>,
    pub ret: Ret,
    #[serde(default)]
    pub lang: Lang,
}

impl Import {
    pub fn new(symbol: impl Into<String>, args: Vec<Param>, ret: Ret) -> Self {
        Self {
            symbol: symbol.into(),
            args,
            ret,
            lang: Lang::C,
        }
    }

    pub fn with_lang(mut self, lang: Lang) -> Self {
        self.lang = lang;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Class {
    pub layout: String,
    #[serde(default)]
    pub base: Option<String>,
    pub methods: BTreeMap<String, Import>,
}

impl Class {
    pub fn insert_method(&mut self, name: impl Into<String>, import: Import) -> Result<()> {
        let name = name.into();
        if self.methods.contains_key(&name) {
            return Err(Error::DuplicateMethod);
        }
        self.methods.insert(name, import);
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ir {
    pub endianness: Endianness,
    #[serde(default)]
    pub bit_order: BitOrder,
    pub layouts: BTreeMap<String, Type>,
    #[serde(default)]
    pub imports: BTreeMap<String, Import>,
    #[serde(default)]
    pub classes: BTreeMap<String, Class>,
}

impl Ir {
    pub fn new(endianness: Endianness) -> Self {
        Self {
            endianness,
            bit_order: BitOrder::Lsb0,
            layouts: BTreeMap::new(),
            imports: BTreeMap::new(),
            classes: BTreeMap::new(),
        }
    }

    pub fn insert(&mut self, name: impl Into<String>, ty: Type) {
        self.layouts.insert(name.into(), ty);
    }

    pub fn insert_import(&mut self, name: impl Into<String>, import: Import) -> Result<()> {
        let name = name.into();
        if self.imports.contains_key(&name) {
            return Err(Error::DuplicateImport);
        }
        self.imports.insert(name, import);
        Ok(())
    }

    pub fn insert_class(&mut self, name: impl Into<String>, class: Class) -> Result<()> {
        let name = name.into();
        if self.classes.contains_key(&name) {
            return Err(Error::DuplicateClass);
        }
        if !self.layouts.contains_key(&class.layout) {
            return Err(Error::UnknownLayout);
        }
        self.classes.insert(name.clone(), class);
        if let Err(e) = self.validate_class_layouts() {
            self.classes.remove(&name);
            return Err(e);
        }
        Ok(())
    }

    pub fn layout(&self, name: &str) -> Result<&Type> {
        self.layouts.get(name).ok_or(Error::UnknownLayout)
    }

    pub fn import(&self, name: &str) -> Result<&Import> {
        self.imports.get(name).ok_or(Error::UnknownImport)
    }

    pub fn class(&self, name: &str) -> Result<&Class> {
        self.classes.get(name).ok_or(Error::UnknownClass)
    }

    pub fn to_json(&self) -> Result<String> {
        serde_json::to_string(self).map_err(|e| Error::Json(e.to_string()))
    }

    pub fn to_json_pretty(&self) -> Result<String> {
        serde_json::to_string_pretty(self).map_err(|e| Error::Json(e.to_string()))
    }

    pub fn from_json(json: &str) -> Result<Self> {
        let ir: Self = serde_json::from_str(json).map_err(|e| Error::Json(e.to_string()))?;
        ir.validate_class_layouts()?;
        Ok(ir)
    }

    pub(crate) fn validate_class_layouts(&self) -> Result<()> {
        for class in self.classes.values() {
            if !self.layouts.contains_key(&class.layout) {
                return Err(Error::UnknownLayout);
            }
        }
        for (name, class) in &self.classes {
            if class.base.is_none() {
                continue;
            }
            self.check_base_chain(name)?;
            let derived_ty = self.layout(&class.layout)?;
            let base_name = class.base.as_deref().unwrap();
            let base_class = self.class(base_name)?;
            let base_ty = self.layout(&base_class.layout)?;
            struct_is_prefix(derived_ty, base_ty)?;
        }
        Ok(())
    }

    fn check_base_chain(&self, start: &str) -> Result<()> {
        let mut seen = BTreeMap::<&str, ()>::new();
        let mut current = start;
        loop {
            if seen.insert(current, ()).is_some() {
                return Err(Error::InvalidLayout);
            }
            let class = self.classes.get(current).ok_or(Error::UnknownClass)?;
            match class.base.as_deref() {
                None => return Ok(()),
                Some(base) => {
                    if !self.classes.contains_key(base) {
                        return Err(Error::UnknownClass);
                    }
                    current = base;
                }
            }
        }
    }
}

/// Derived struct fields `[0..n]` must equal the base struct (name, ty, offset, size, align).
fn struct_is_prefix(derived: &Type, base: &Type) -> Result<()> {
    let (Type::Struct(derived), Type::Struct(base)) = (derived, base) else {
        return Err(Error::InvalidLayout);
    };
    if derived.fields.len() < base.fields.len() {
        return Err(Error::InvalidLayout);
    }
    if derived.fields[..base.fields.len()] != base.fields[..] {
        return Err(Error::InvalidLayout);
    }
    for suffix in &derived.fields[base.fields.len()..] {
        if base.fields.iter().any(|f| f.name == suffix.name) {
            return Err(Error::DuplicateField);
        }
    }
    Ok(())
}

pub(crate) fn align_up(value: u64, align: u64) -> Result<u64> {
    if align == 0 || !align.is_power_of_two() {
        return Err(Error::BadAlign);
    }
    let mask = align - 1;
    if value > u64::MAX - mask {
        return Err(Error::Overflow);
    }
    Ok((value + mask) & !mask)
}
