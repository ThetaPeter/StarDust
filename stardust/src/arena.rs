use crate::error::{Error, Result};
use crate::ir::{align_up, Type};
use crate::region::Region;

/// Host-only bump cursor over a region. The cursor is not stored in the region.
/// Offset `0` is never allocated (null). The first object is at offset `8`.
pub struct Bump<'a> {
    region: &'a mut Region,
    cursor: u64,
}

impl<'a> Bump<'a> {
    pub fn new(region: &'a mut Region) -> Self {
        Self { region, cursor: 8 }
    }

    /// Place `ty` at `align_up(cursor, ty.align())`. Does not write bytes.
    /// Fails with [`Error::Alloc`] if the object would not fit in the region.
    pub fn alloc(&mut self, ty: &Type) -> Result<u64> {
        let offset = align_up(self.cursor, ty.align())?;
        let size = ty.inline_size();
        let end = offset.checked_add(size).ok_or(Error::Overflow)?;
        let region_size = u64::try_from(self.region.size()).map_err(|_| Error::Overflow)?;
        if end > region_size {
            return Err(Error::Alloc);
        }
        self.cursor = end;
        Ok(offset)
    }
}
