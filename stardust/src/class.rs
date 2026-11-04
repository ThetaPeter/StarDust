use std::collections::{BTreeMap, BTreeSet};
#[cfg(feature = "python")]
use std::path::PathBuf;

use crate::arena::Bump;
use crate::error::{Error, Result};
use crate::ir::{BitOrder, Endianness, ExportField, Import, Ir, Lang, Param, Ret, Type};
use crate::loader::{CallArg, CallRet, ImportFn, Library};
use crate::path::Path;
#[cfg(feature = "python")]
use crate::python::call_python;
use crate::region::Region;
use crate::value::Value;

/// Per-method host loader. Not stored in dumpable IR.
pub enum MethodLoader<'a> {
    Library(&'a Library),
    #[cfg(feature = "python")]
    Python {
        script: PathBuf,
    },
}

enum BoundMethod<'a> {
    Library(ImportFn<'a>),
    #[cfg(feature = "python")]
    Python {
        script: PathBuf,
        symbol: String,
    },
}

struct BoundExportField<'a> {
    value_size: u64,
    assign: ImportFn<'a>,
    retrieve: ImportFn<'a>,
}

/// Class methods bound to I1 `Library` and/or I3 Python scripts.
///
/// [`BoundClass::call`] fills `CallArg::Ptr { region: 0, offset }` and
/// `CallArg::Len(layout.inline_size())` (E2a, one region). [`BoundClass::call_with`]
/// appends extra same-region `Scalar` / `Ptr` args (O6). The host does not
/// copy the instance span between methods.
pub struct BoundClass<'a> {
    layout: Type,
    inline_size: u64,
    methods: BTreeMap<String, BoundMethod<'a>>,
    local_fields: BTreeMap<String, BoundExportField<'a>>,
    export_fields: BTreeMap<String, BoundExportField<'a>>,
    export_methods: BTreeMap<String, BoundMethod<'a>>,
    endianness: Endianness,
    bit_order: BitOrder,
}

/// Instance of a [`BoundClass`] at one region offset, plus named `link` slots.
pub struct Handle<'a> {
    class: &'a BoundClass<'a>,
    offset: u64,
    links: BTreeMap<String, (&'a BoundClass<'a>, u64)>,
}

impl<'a> BoundClass<'a> {
    pub fn bind(
        ir: &Ir,
        class_name: &str,
        loaders: &BTreeMap<String, MethodLoader<'a>>,
    ) -> Result<Self> {
        let class = ir.class(class_name)?;
        let layout = ir.layout(&class.layout)?;
        let inline_size = layout.inline_size();

        let mut methods = BTreeMap::new();
        for (name, import) in merged_methods(ir, class_name)? {
            let loader = loaders
                .get(&name)
                .ok_or_else(|| Error::MissingLoader(name.clone()))?;
            methods.insert(name, bind_method(&import, loader)?);
        }
        let mut local_fields = BTreeMap::new();
        for (name, field) in merged_local_fields(ir, class_name)? {
            if methods.contains_key(&name) {
                return Err(Error::DuplicateMethod);
            }
            let loader = loaders
                .get(&name)
                .ok_or_else(|| Error::MissingLoader(name.clone()))?;
            local_fields.insert(name, bind_export_field(ir, &field, loader)?);
        }
        let mut export_fields = BTreeMap::new();
        for (name, field) in &class.export_fields {
            let loader = loaders
                .get(name)
                .ok_or_else(|| Error::MissingLoader(name.clone()))?;
            export_fields.insert(name.clone(), bind_export_field(ir, field, loader)?);
        }
        let mut export_methods = BTreeMap::new();
        for (name, import) in &class.export_methods {
            let loader = loaders
                .get(name)
                .ok_or_else(|| Error::MissingLoader(name.clone()))?;
            export_methods.insert(name.clone(), bind_export_method(import, loader)?);
        }
        Ok(Self {
            layout: layout.clone(),
            inline_size,
            methods,
            local_fields,
            export_fields,
            export_methods,
            endianness: ir.endianness,
            bit_order: ir.bit_order,
        })
    }

    /// Bump-allocate an instance of this class's layout in `bump`. Does not write bytes.
    /// Ordinary placement remains caller-chosen `(region, offset)` for [`Self::call`].
    pub fn place(&self, bump: &mut Bump<'_>) -> Result<u64> {
        bump.alloc(&self.layout)
    }

    pub fn handle(&'a self, offset: u64) -> Handle<'a> {
        Handle {
            class: self,
            offset,
            links: BTreeMap::new(),
        }
    }

    pub fn call(&self, method: &str, region: &mut Region, offset: u64) -> Result<CallRet> {
        self.call_with(method, region, offset, &[])
    }

    /// Like [`Self::call`], then `extra` after the instance `(Ptr, Len)`.
    ///
    /// `extra` may be [`CallArg::Scalar`] and [`CallArg::Ptr`] with `region: 0`
    /// only. [`CallArg::Len`] or a `Ptr` into another region is an error.
    pub fn call_with(
        &self,
        method: &str,
        region: &mut Region,
        offset: u64,
        extra: &[CallArg],
    ) -> Result<CallRet> {
        if self.export_fields.contains_key(method)
            || self.export_methods.contains_key(method)
            || self.local_fields.contains_key(method)
        {
            return Err(Error::UnknownMethod);
        }
        check_class_extra(extra)?;
        match self.methods.get(method) {
            None => Err(Error::UnknownMethod),
            Some(BoundMethod::Library(func)) => {
                let mut args = Vec::with_capacity(2 + extra.len());
                args.push(CallArg::Ptr { region: 0, offset });
                args.push(CallArg::Len(self.inline_size));
                args.extend_from_slice(extra);
                func.call(region, &args)
            }
            #[cfg(feature = "python")]
            Some(BoundMethod::Python { script, symbol }) => {
                if !extra.is_empty() {
                    return Err(Error::UnsupportedCall);
                }
                call_python(region, offset, self.inline_size, script, symbol)?;
                Ok(CallRet::Void)
            }
        }
    }
}

impl<'a> Handle<'a> {
    pub fn offset(&self) -> u64 {
        self.offset
    }

    /// Write `db.offset` into this instance's named `link` field `slot`.
    pub fn link(&mut self, slot: &str, db: &Handle<'a>, region: &mut Region) -> Result<()> {
        if db.offset == 0 {
            return Err(Error::Unlinked);
        }
        ensure_link_field(&self.class.layout, slot)?;
        let incoming: BTreeSet<&str> = db
            .class
            .export_fields
            .keys()
            .chain(db.class.export_methods.keys())
            .map(|s| s.as_str())
            .collect();
        for name in &incoming {
            if self.class.local_fields.contains_key(*name) {
                return Err(Error::DuplicateExport);
            }
        }
        for (existing_slot, (existing, _)) in &self.links {
            if existing_slot == slot {
                continue;
            }
            for name in existing
                .export_fields
                .keys()
                .chain(existing.export_methods.keys())
            {
                if incoming.contains(name.as_str()) {
                    return Err(Error::DuplicateExport);
                }
            }
        }
        crate::access::set_at(
            &self.class.layout,
            self.class.endianness,
            self.class.bit_order,
            region.as_bytes_mut(),
            self.offset,
            &Path::new().name(slot),
            &Value::U64(db.offset),
        )?;
        self.links.insert(slot.to_string(), (db.class, db.offset));
        Ok(())
    }

    pub fn field_assign(
        &self,
        name: &str,
        region: &mut Region,
        value_offset: u64,
        value_len: u64,
    ) -> Result<()> {
        self.export_field_call(name, region, value_offset, value_len, true)
    }

    pub fn field_retrieve(
        &self,
        name: &str,
        region: &mut Region,
        value_offset: u64,
        value_len: u64,
    ) -> Result<()> {
        self.export_field_call(name, region, value_offset, value_len, false)
    }

    pub fn call_export(&self, name: &str, region: &mut Region) -> Result<CallRet> {
        let (db, db_off) = self.linked_db_for_method(name, region)?;
        let func = db.export_methods.get(name).ok_or(Error::UnknownExport)?;
        match func {
            BoundMethod::Library(func) => func.call(
                region,
                &[
                    CallArg::Ptr {
                        region: 0,
                        offset: db_off,
                    },
                    CallArg::Len(db.inline_size),
                ],
            ),
            #[cfg(feature = "python")]
            BoundMethod::Python { .. } => Err(Error::TypeMismatch),
        }
    }

    fn export_field_call(
        &self,
        name: &str,
        region: &mut Region,
        value_offset: u64,
        value_len: u64,
        assign: bool,
    ) -> Result<()> {
        if let Some(field) = self.class.local_fields.get(name) {
            return self.field_call(
                field,
                region,
                self.offset,
                self.class.inline_size,
                value_offset,
                value_len,
                assign,
            );
        }
        let (db, db_off, field) = self.linked_db_for_field(name, region)?;
        self.field_call(
            field,
            region,
            db_off,
            db.inline_size,
            value_offset,
            value_len,
            assign,
        )
    }

    fn field_call(
        &self,
        field: &BoundExportField<'a>,
        region: &mut Region,
        obj_off: u64,
        obj_len: u64,
        value_offset: u64,
        value_len: u64,
        assign: bool,
    ) -> Result<()> {
        if value_len != field.value_size {
            return Err(Error::TypeMismatch);
        }
        check_region_span(region, obj_off, obj_len)?;
        check_region_span(region, value_offset, value_len)?;
        let func = if assign {
            &field.assign
        } else {
            &field.retrieve
        };
        func.call(
            region,
            &[
                CallArg::Ptr {
                    region: 0,
                    offset: obj_off,
                },
                CallArg::Len(obj_len),
                CallArg::Ptr {
                    region: 0,
                    offset: value_offset,
                },
                CallArg::Len(value_len),
            ],
        )?;
        Ok(())
    }

    fn linked_db_for_field(
        &self,
        name: &str,
        region: &Region,
    ) -> Result<(&'a BoundClass<'a>, u64, &BoundExportField<'a>)> {
        for (slot, (db, recorded)) in &self.links {
            if let Some(field) = db.export_fields.get(name) {
                let db_off = self.read_link(slot, region)?;
                if db_off == 0 {
                    return Err(Error::Unlinked);
                }
                let _ = recorded;
                return Ok((*db, db_off, field));
            }
        }
        Err(Error::UnknownExport)
    }

    fn linked_db_for_method(
        &self,
        name: &str,
        region: &Region,
    ) -> Result<(&'a BoundClass<'a>, u64)> {
        for (slot, (db, _)) in &self.links {
            if db.export_methods.contains_key(name) {
                let db_off = self.read_link(slot, region)?;
                if db_off == 0 {
                    return Err(Error::Unlinked);
                }
                return Ok((*db, db_off));
            }
        }
        Err(Error::UnknownExport)
    }

    fn read_link(&self, slot: &str, region: &Region) -> Result<u64> {
        let val = crate::access::get_at(
            &self.class.layout,
            self.class.endianness,
            self.class.bit_order,
            region.as_bytes(),
            self.offset,
            &Path::new().name(slot),
        )?;
        match val {
            Value::U64(off) => Ok(off),
            _ => Err(Error::TypeMismatch),
        }
    }
}

fn ensure_link_field(layout: &Type, slot: &str) -> Result<()> {
    let Type::Struct(st) = layout else {
        return Err(Error::InvalidLayout);
    };
    let field = st
        .fields
        .iter()
        .find(|f| f.name == slot)
        .ok_or(Error::UnknownPath)?;
    if !matches!(field.ty, Type::Link) {
        return Err(Error::TypeMismatch);
    }
    Ok(())
}

fn check_region_span(region: &Region, offset: u64, len: u64) -> Result<()> {
    let end = offset.checked_add(len).ok_or(Error::Overflow)?;
    let size = u64::try_from(region.size()).map_err(|_| Error::OutOfBounds)?;
    if offset > size || end > size {
        Err(Error::OutOfBounds)
    } else {
        Ok(())
    }
}

fn check_class_extra(extra: &[CallArg]) -> Result<()> {
    for arg in extra {
        match arg {
            CallArg::Scalar(_) => {}
            CallArg::Ptr { region: 0, .. } => {}
            CallArg::Ptr { .. } => return Err(Error::TypeMismatch),
            CallArg::Len(_) => return Err(Error::UnsupportedCall),
        }
    }
    Ok(())
}

/// Walk the base chain root-first; later classes overwrite method names.
fn merged_methods(ir: &Ir, class_name: &str) -> Result<BTreeMap<String, Import>> {
    let mut chain = Vec::new();
    let mut seen = BTreeSet::new();
    let mut name = class_name;
    loop {
        if !seen.insert(name.to_string()) {
            return Err(Error::InvalidLayout);
        }
        let class = ir.class(name)?;
        chain.push(class);
        match class.base.as_deref() {
            None => break,
            Some(base) => name = base,
        }
    }
    let mut methods = BTreeMap::new();
    for class in chain.into_iter().rev() {
        for (method, import) in &class.methods {
            methods.insert(method.clone(), import.clone());
        }
    }
    Ok(methods)
}

/// Walk the base chain root-first; later classes overwrite local-field names.
pub(crate) fn merged_local_fields(
    ir: &Ir,
    class_name: &str,
) -> Result<BTreeMap<String, ExportField>> {
    let mut chain = Vec::new();
    let mut seen = BTreeSet::new();
    let mut name = class_name;
    loop {
        if !seen.insert(name.to_string()) {
            return Err(Error::InvalidLayout);
        }
        let class = ir.class(name)?;
        chain.push(class);
        match class.base.as_deref() {
            None => break,
            Some(base) => name = base,
        }
    }
    let mut fields = BTreeMap::new();
    for class in chain.into_iter().rev() {
        for (field_name, field) in &class.local_fields {
            fields.insert(field_name.clone(), field.clone());
        }
    }
    Ok(fields)
}

fn bind_method<'a>(import: &Import, loader: &MethodLoader<'a>) -> Result<BoundMethod<'a>> {
    match import.lang {
        Lang::Python => bind_python(import, loader),
        Lang::C | Lang::Llvm | Lang::Haskell | Lang::Rust => bind_library(import, loader),
    }
}

fn bind_export_field<'a>(
    ir: &Ir,
    field: &ExportField,
    loader: &MethodLoader<'a>,
) -> Result<BoundExportField<'a>> {
    if matches!(field.assign.lang, Lang::Python) || matches!(field.retrieve.lang, Lang::Python) {
        return Err(Error::TypeMismatch);
    }
    check_export_field_sig(&field.assign)?;
    check_export_field_sig(&field.retrieve)?;
    let value_size = ir.layout(&field.layout)?.inline_size();
    match loader {
        MethodLoader::Library(lib) => Ok(BoundExportField {
            value_size,
            assign: lib.bind_import(&field.assign)?,
            retrieve: lib.bind_import(&field.retrieve)?,
        }),
        #[cfg(feature = "python")]
        MethodLoader::Python { .. } => Err(Error::TypeMismatch),
    }
}

fn bind_export_method<'a>(import: &Import, loader: &MethodLoader<'a>) -> Result<BoundMethod<'a>> {
    if matches!(import.lang, Lang::Python) {
        return Err(Error::TypeMismatch);
    }
    check_export_method_sig(import)?;
    bind_library(import, loader)
}

fn bind_library<'a>(import: &Import, loader: &MethodLoader<'a>) -> Result<BoundMethod<'a>> {
    match loader {
        MethodLoader::Library(lib) => {
            check_library_sig(import)?;
            Ok(BoundMethod::Library(lib.bind_import(import)?))
        }
        #[cfg(feature = "python")]
        MethodLoader::Python { .. } => Err(Error::TypeMismatch),
    }
}

#[cfg(feature = "python")]
fn bind_python<'a>(import: &Import, loader: &MethodLoader<'a>) -> Result<BoundMethod<'a>> {
    match loader {
        MethodLoader::Python { script } => {
            check_python_sig(import)?;
            Ok(BoundMethod::Python {
                script: script.clone(),
                symbol: import.symbol.clone(),
            })
        }
        MethodLoader::Library(_) => Err(Error::TypeMismatch),
    }
}

#[cfg(not(feature = "python"))]
fn bind_python<'a>(_import: &Import, _loader: &MethodLoader<'a>) -> Result<BoundMethod<'a>> {
    Err(Error::Unsupported { family: "python" })
}

pub(crate) fn check_export_field_sig(import: &Import) -> Result<()> {
    if matches!(import.ret, Ret::OwnedPtr)
        || import.args.iter().any(|p| matches!(p, Param::OwnedPtr))
    {
        return Err(Error::IncompatibleImport);
    }
    const ARGS: [Param; 4] = [Param::Ptr, Param::Len, Param::Ptr, Param::Len];
    if import.args.as_slice() == ARGS && matches!(import.ret, Ret::Void) {
        Ok(())
    } else {
        Err(Error::UnsupportedCall)
    }
}

pub(crate) fn check_export_method_sig(import: &Import) -> Result<()> {
    if matches!(import.ret, Ret::OwnedPtr)
        || import.args.iter().any(|p| matches!(p, Param::OwnedPtr))
    {
        return Err(Error::IncompatibleImport);
    }
    const ARGS: [Param; 2] = [Param::Ptr, Param::Len];
    if import.args.as_slice() == ARGS && matches!(import.ret, Ret::Void) {
        Ok(())
    } else {
        Err(Error::UnsupportedCall)
    }
}

fn check_library_sig(import: &Import) -> Result<()> {
    if matches!(import.ret, Ret::OwnedPtr)
        || import.args.iter().any(|p| matches!(p, Param::OwnedPtr))
    {
        return Err(Error::IncompatibleImport);
    }
    match import.args.as_slice() {
        [Param::Ptr, Param::Len] => Ok(()),
        [Param::Ptr, Param::Len, rest @ ..] => {
            if rest
                .iter()
                .all(|p| matches!(p, Param::Scalar(_) | Param::Ptr))
            {
                Ok(())
            } else {
                Err(Error::UnsupportedCall)
            }
        }
        _ => Err(Error::IncompatibleImport),
    }
}

#[cfg(feature = "python")]
fn check_python_sig(import: &Import) -> Result<()> {
    if !matches!(import.ret, Ret::Void) || import.args.as_slice() != [Param::Ptr, Param::Len] {
        return Err(Error::UnsupportedCall);
    }
    Ok(())
}
