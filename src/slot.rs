use std::collections::BTreeMap;

use crate::error::{Error, Result};
use crate::ir::{align_up, Type};
use crate::region::Region;

/// Host-only slot allocator over a region. Live and free maps are not stored in the region.
/// Offset `0` is never allocated (null). The first object is at offset `8`.
pub struct SlotAlloc<'a> {
    region: &'a mut Region,
    cursor: u64,
    live: BTreeMap<u64, u64>,
    free: BTreeMap<u64, u64>,
}

impl<'a> SlotAlloc<'a> {
    pub fn new(region: &'a mut Region) -> Self {
        Self {
            region,
            cursor: 8,
            live: BTreeMap::new(),
            free: BTreeMap::new(),
        }
    }

    /// Place `ty` on the lowest-offset freed slot whose size is `ty.inline_size()` and whose
    /// offset is already aligned for `ty`. Otherwise bump from the high-water cursor with the
    /// same placement rules as [`crate::Bump`]. Does not write bytes.
    /// Fails with [`Error::Alloc`] if bump would not fit in the region.
    pub fn alloc(&mut self, ty: &Type) -> Result<u64> {
        let size = ty.inline_size();
        let align = ty.align();
        let reuse = self.free.iter().find_map(|(&offset, &slot_size)| {
            if slot_size != size {
                return None;
            }
            match align_up(offset, align) {
                Ok(aligned) if aligned == offset => Some(offset),
                _ => None,
            }
        });
        if let Some(offset) = reuse {
            self.free.remove(&offset);
            self.live.insert(offset, size);
            return Ok(offset);
        }

        let offset = align_up(self.cursor, align)?;
        let end = offset.checked_add(size).ok_or(Error::Overflow)?;
        let region_size = u64::try_from(self.region.size()).map_err(|_| Error::Overflow)?;
        if end > region_size {
            return Err(Error::Alloc);
        }
        self.cursor = end;
        self.live.insert(offset, size);
        Ok(offset)
    }

    /// Return `offset` to the free map. Unknown or double-free is [`Error::OutOfBounds`].
    /// Offset `0` is never live.
    pub fn free(&mut self, offset: u64) -> Result<()> {
        let size = self.live.remove(&offset).ok_or(Error::OutOfBounds)?;
        self.free.insert(offset, size);
        Ok(())
    }
}
