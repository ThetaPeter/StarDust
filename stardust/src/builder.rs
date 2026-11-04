use crate::error::{Error, Result};
use crate::ir::{align_up, BitField, BitpackType, ColumnarType, Field, Scalar, StructType, Type};

#[derive(Debug, Clone, Default)]
pub struct StructBuilder {
    fields: Vec<(String, Type)>,
}

impl StructBuilder {
    pub fn new() -> Self {
        Self { fields: Vec::new() }
    }

    pub fn field(mut self, name: impl Into<String>, ty: Type) -> Self {
        self.fields.push((name.into(), ty));
        self
    }

    /// C-like placement: each field offset is aligned to that field's align;
    /// struct align is the max field align; struct size is rounded up to struct align.
    pub fn repr_c(self) -> Result<Type> {
        let mut seen = std::collections::BTreeSet::new();
        let mut offset = 0u64;
        let mut max_align = 1u64;
        let mut fields = Vec::with_capacity(self.fields.len());

        for (name, ty) in self.fields {
            if !seen.insert(name.clone()) {
                return Err(Error::DuplicateField);
            }
            if matches!(ty, Type::Bitpack(_)) {
                return Err(Error::InvalidLayout);
            }
            let f_align = ty.align();
            let f_size = ty.inline_size();
            if f_align == 0 || !f_align.is_power_of_two() {
                return Err(Error::BadAlign);
            }
            offset = align_up(offset, f_align)?;
            fields.push(Field {
                name,
                ty,
                offset,
                size: f_size,
                align: f_align,
            });
            offset = offset.checked_add(f_size).ok_or(Error::Overflow)?;
            max_align = max_align.max(f_align);
        }

        let size = align_up(offset, max_align)?;
        Ok(Type::Struct(StructType {
            fields,
            size,
            align: max_align,
        }))
    }
}

#[derive(Debug, Clone)]
pub struct ColumnarBuilder {
    capacity: u64,
    columns: Vec<(String, Type)>,
}

impl ColumnarBuilder {
    pub fn new(capacity: u64) -> Self {
        Self {
            capacity,
            columns: Vec::new(),
        }
    }

    pub fn column(mut self, name: impl Into<String>, ty: Type) -> Result<Self> {
        let name = name.into();
        if self.columns.iter().any(|(n, _)| n == &name) {
            return Err(Error::DuplicateField);
        }
        let cell = canonicalize_column_cell(ty)?;
        self.columns.push((name, cell));
        Ok(self)
    }

    /// Count occupies `[0, 8)` (align 8). Each column is a contiguous array of
    /// `capacity` cells; offsets are independent of live row count.
    pub fn build(self) -> Result<Type> {
        let mut next = 8u64;
        let mut table_align = 8u64;
        let mut columns = Vec::with_capacity(self.columns.len());

        for (name, ty) in self.columns {
            let cell_align = ty.align();
            let cell_size = ty.inline_size();
            if cell_align == 0 || !cell_align.is_power_of_two() {
                return Err(Error::BadAlign);
            }
            let offset = align_up(next, cell_align)?;
            let size = cell_size
                .checked_mul(self.capacity)
                .ok_or(Error::Overflow)?;
            columns.push(Field {
                name,
                ty,
                offset,
                size,
                align: cell_align,
            });
            next = offset.checked_add(size).ok_or(Error::Overflow)?;
            table_align = table_align.max(cell_align);
        }

        let size = align_up(next, table_align)?;
        Ok(Type::Columnar(ColumnarType {
            capacity: self.capacity,
            columns,
            size,
            align: table_align,
        }))
    }
}

fn canonicalize_column_cell(ty: Type) -> Result<Type> {
    match ty {
        Type::Scalar(s) => Ok(Type::scalar(s)),
        Type::Array(a) => match a.elem.as_ref() {
            Type::Scalar(s) => Type::array(Type::scalar(*s), a.count),
            _ => Err(Error::BadColumnType),
        },
        _ => Err(Error::BadColumnType),
    }
}

#[derive(Debug, Clone)]
pub struct BitpackBuilder {
    container: Option<Scalar>,
    span: u64,
    fields: Vec<(String, u64)>,
}

impl BitpackBuilder {
    pub fn new(container: Scalar) -> Self {
        Self {
            container: Some(container),
            span: container.size(),
            fields: Vec::new(),
        }
    }

    /// Span-only bitstream (L6). `n` is the byte length; `n == 0` is `Overflow`.
    pub fn span(n: u64) -> Result<Self> {
        if n == 0 {
            return Err(Error::Overflow);
        }
        Ok(Self {
            container: None,
            span: n,
            fields: Vec::new(),
        })
    }

    pub fn field(mut self, name: impl Into<String>, width: u64) -> Self {
        self.fields.push((name.into(), width));
        self
    }

    /// Place fields in declaration order. Field `i` starts at `sum(widths[..i])`.
    /// L5: `sum(widths)` must be `≤ 8 * size(container)`; unused high bits allowed.
    /// L6: `sum(widths)` must be `≤ 8 * span`; each width is `1..=64`.
    pub fn build(self) -> Result<Type> {
        let (container, span, align) = match self.container {
            Some(c) => {
                match c {
                    Scalar::U8 | Scalar::U16 | Scalar::U32 | Scalar::U64 => {}
                    _ => return Err(Error::InvalidLayout),
                }
                (Some(c), c.size(), c.align())
            }
            None => {
                if self.span == 0 {
                    return Err(Error::Overflow);
                }
                (None, self.span, 1u64)
            }
        };
        let cap_bits = span.checked_mul(8).ok_or(Error::Overflow)?;
        let mut seen = std::collections::BTreeSet::new();
        let mut bit_offset = 0u64;
        let mut fields = Vec::with_capacity(self.fields.len());

        for (name, width) in self.fields {
            if !seen.insert(name.clone()) {
                return Err(Error::DuplicateField);
            }
            if width == 0 || width > 64 {
                return Err(Error::Overflow);
            }
            let end = bit_offset.checked_add(width).ok_or(Error::Overflow)?;
            if end > cap_bits {
                return Err(Error::Overflow);
            }
            fields.push(BitField {
                name,
                width,
                bit_offset,
            });
            bit_offset = end;
        }

        Ok(Type::Bitpack(BitpackType {
            container,
            span,
            fields,
            size: span,
            align,
        }))
    }
}
