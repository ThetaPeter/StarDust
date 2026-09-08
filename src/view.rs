use std::mem::{align_of, size_of};
use std::slice;

use crate::error::{Error, Result};
use crate::ir::{BitOrder, ColumnarType, Endianness, Ir, Scalar, Type};
use crate::path::Path;
use crate::region::Region;
use crate::value::Value;

pub struct View<'a> {
    ty: &'a Type,
    endian: Endianness,
    bit_order: BitOrder,
    bytes: &'a mut [u8],
}

impl<'a> View<'a> {
    pub fn new(ty: &'a Type, endian: Endianness, bytes: &'a mut [u8]) -> Result<Self> {
        Self::from_parts(ty, endian, BitOrder::Lsb0, bytes)
    }

    fn from_parts(
        ty: &'a Type,
        endian: Endianness,
        bit_order: BitOrder,
        bytes: &'a mut [u8],
    ) -> Result<Self> {
        if endian != Endianness::Little {
            return Err(Error::UnsupportedEndian);
        }
        if bit_order != BitOrder::Lsb0 {
            return Err(Error::UnsupportedBitOrder);
        }
        Ok(Self {
            ty,
            endian,
            bit_order,
            bytes,
        })
    }

    pub fn ty(&self) -> &Type {
        self.ty
    }

    pub fn as_ptr(&self) -> *const u8 {
        self.bytes.as_ptr()
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.bytes
    }

    pub fn get(&self, path: &Path) -> Result<Value> {
        crate::access::get(self.ty, self.endian, self.bit_order, self.bytes, path)
    }

    pub fn set(&mut self, path: &Path, value: &Value) -> Result<()> {
        crate::access::set(
            self.ty,
            self.endian,
            self.bit_order,
            self.bytes,
            path,
            value,
        )
    }
}

impl Ir {
    pub fn retrieve(&self, layout: &str, region: &Region, path: &Path) -> Result<Value> {
        self.retrieve_at(layout, region, 0, path)
    }

    pub fn retrieve_at(
        &self,
        layout: &str,
        region: &Region,
        offset: u64,
        path: &Path,
    ) -> Result<Value> {
        let ty = self.layout(layout)?;
        crate::access::get_at(
            ty,
            self.endianness,
            self.bit_order,
            region.as_bytes(),
            offset,
            path,
        )
    }

    pub fn store(
        &self,
        layout: &str,
        region: &mut Region,
        path: &Path,
        value: &Value,
    ) -> Result<()> {
        self.store_at(layout, region, 0, path, value)
    }

    pub fn store_at(
        &self,
        layout: &str,
        region: &mut Region,
        offset: u64,
        path: &Path,
        value: &Value,
    ) -> Result<()> {
        let ty = self.layout(layout)?;
        crate::access::set_at(
            ty,
            self.endianness,
            self.bit_order,
            region.as_bytes_mut(),
            offset,
            path,
            value,
        )
    }

    pub fn view<'a>(&'a self, layout: &str, region: &'a mut Region) -> Result<View<'a>> {
        let ty = self.layout(layout)?;
        if matches!(ty, Type::Columnar(_)) {
            return Err(Error::TypeMismatch);
        }
        View::from_parts(ty, self.endianness, self.bit_order, region.as_bytes_mut())
    }

    pub fn column_view<'a>(
        &'a self,
        layout: &str,
        region: &'a mut Region,
    ) -> Result<ColumnView<'a>> {
        if self.endianness != Endianness::Little {
            return Err(Error::UnsupportedEndian);
        }
        if self.bit_order != BitOrder::Lsb0 {
            return Err(Error::UnsupportedBitOrder);
        }
        let ty = self.layout(layout)?;
        let Type::Columnar(col) = ty else {
            return Err(Error::TypeMismatch);
        };
        if col.size < 8 {
            return Err(Error::OutOfBounds);
        }
        let need = usize::try_from(col.size).map_err(|_| Error::Overflow)?;
        if region.size() < need {
            return Err(Error::OutOfBounds);
        }
        let bytes = &mut region.as_bytes_mut()[..need];
        let view = ColumnView { layout: col, bytes };
        if view.row_count() > col.capacity {
            return Err(Error::OutOfBounds);
        }
        Ok(view)
    }
}

mod column_cell_seal {
    pub trait Sealed {}
}

/// Maps a Rust cell type to a W12 column cell (`scalar` or `[scalar; K]`).
pub trait ColumnCell: column_cell_seal::Sealed + Sized {
    #[doc(hidden)]
    fn matches_cell(ty: &Type) -> bool;
}

macro_rules! impl_scalar_column_cell {
    ($($rust:ty => $var:ident),+ $(,)?) => {
        $(
            impl column_cell_seal::Sealed for $rust {}
            impl ColumnCell for $rust {
                fn matches_cell(ty: &Type) -> bool {
                    matches!(ty, Type::Scalar(Scalar::$var))
                }
            }
        )+
    };
}

impl_scalar_column_cell!(
    i8 => I8,
    u8 => U8,
    i16 => I16,
    u16 => U16,
    i32 => I32,
    u32 => U32,
    i64 => I64,
    u64 => U64,
    f32 => F32,
    f64 => F64,
);

impl<T: ColumnCell, const K: usize> column_cell_seal::Sealed for [T; K] {}

impl<T: ColumnCell, const K: usize> ColumnCell for [T; K] {
    fn matches_cell(ty: &Type) -> bool {
        match ty {
            Type::Array(a) if a.count == K as u64 => T::matches_cell(a.elem.as_ref()),
            _ => false,
        }
    }
}

pub struct ColumnView<'a> {
    layout: &'a ColumnarType,
    bytes: &'a mut [u8],
}

impl<'a> ColumnView<'a> {
    pub fn row_count(&self) -> u64 {
        let mut buf = [0u8; 8];
        buf.copy_from_slice(&self.bytes[..8]);
        u64::from_le_bytes(buf)
    }

    pub fn set_row_count(&mut self, n: u64) -> Result<()> {
        if n > self.layout.capacity {
            return Err(Error::OutOfBounds);
        }
        self.bytes[..8].copy_from_slice(&n.to_le_bytes());
        Ok(())
    }

    pub fn column_mut<T: ColumnCell>(&mut self, name: &str) -> Result<&mut [T]> {
        let (offset, cell_size, cell_align) = {
            let field = self
                .layout
                .columns
                .iter()
                .find(|f| f.name == *name)
                .ok_or(Error::UnknownPath)?;
            if !T::matches_cell(&field.ty) {
                return Err(Error::TypeMismatch);
            }
            (field.offset, field.ty.inline_size(), field.ty.align())
        };
        if size_of::<T>() as u64 != cell_size {
            return Err(Error::TypeMismatch);
        }
        if align_of::<T>() as u64 != cell_align {
            return Err(Error::BadAlign);
        }
        let live = self.row_count();
        if live > self.layout.capacity {
            return Err(Error::OutOfBounds);
        }
        let start = usize::try_from(offset).map_err(|_| Error::OutOfBounds)?;
        let len = usize::try_from(live).map_err(|_| Error::Overflow)?;
        let byte_len = len.checked_mul(size_of::<T>()).ok_or(Error::Overflow)?;
        let end = start.checked_add(byte_len).ok_or(Error::Overflow)?;
        if end > self.bytes.len() {
            return Err(Error::OutOfBounds);
        }
        let ptr = unsafe { self.bytes.as_mut_ptr().add(start) };
        if (ptr as usize) % align_of::<T>() != 0 {
            return Err(Error::BadAlign);
        }
        // SAFETY: `T` matches the column cell (size and align checked). `ptr` is
        // `region_base + column.offset`, aligned for `T`. The live prefix is
        // inside the baked column span; the region borrow is exclusive.
        Ok(unsafe { slice::from_raw_parts_mut(ptr.cast::<T>(), len) })
    }
}
