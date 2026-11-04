use crate::error::{Error, Result};
use crate::ir::{BitField, BitOrder, Endianness, Scalar, Type};
use crate::path::{Path, Seg};
use crate::value::Value;

struct Cursor<'a> {
    ty: &'a Type,
    offset: u64,
    bit_field: Option<&'a BitField>,
}

pub fn get(
    ty: &Type,
    endian: Endianness,
    bit_order: BitOrder,
    bytes: &[u8],
    path: &Path,
) -> Result<Value> {
    get_at(ty, endian, bit_order, bytes, 0, path)
}

pub fn set(
    ty: &Type,
    endian: Endianness,
    bit_order: BitOrder,
    bytes: &mut [u8],
    path: &Path,
    value: &Value,
) -> Result<()> {
    set_at(ty, endian, bit_order, bytes, 0, path, value)
}

pub(crate) fn get_at(
    ty: &Type,
    endian: Endianness,
    bit_order: BitOrder,
    bytes: &[u8],
    base: u64,
    path: &Path,
) -> Result<Value> {
    require_little(endian)?;
    require_lsb0(bit_order)?;
    let cur = walk_from(ty, bytes, path, base)?;
    if let Some(field) = cur.bit_field {
        read_bit_field(cur.ty, field, cur.offset, bytes)
    } else {
        read_value(cur.ty, cur.offset, bytes)
    }
}

pub(crate) fn set_at(
    ty: &Type,
    endian: Endianness,
    bit_order: BitOrder,
    bytes: &mut [u8],
    base: u64,
    path: &Path,
    value: &Value,
) -> Result<()> {
    require_little(endian)?;
    require_lsb0(bit_order)?;
    let cur = walk_from(ty, bytes, path, base)?;
    if let Some(field) = cur.bit_field {
        write_bit_field(cur.ty, field, cur.offset, bytes, value)
    } else {
        write_value(cur.ty, cur.offset, bytes, value)
    }
}

fn require_little(endian: Endianness) -> Result<()> {
    match endian {
        Endianness::Little => Ok(()),
        Endianness::Big => Err(Error::UnsupportedEndian),
    }
}

fn require_lsb0(bit_order: BitOrder) -> Result<()> {
    match bit_order {
        BitOrder::Lsb0 => Ok(()),
        BitOrder::Msb0 => Err(Error::UnsupportedBitOrder),
    }
}

fn walk_from<'a>(root: &'a Type, bytes: &[u8], path: &Path, base: u64) -> Result<Cursor<'a>> {
    let mut ty = root;
    let mut offset = base;
    let mut bit_field = None;
    for seg in &path.segs {
        if bit_field.is_some() {
            return Err(Error::UnknownPath);
        }
        match (seg, ty) {
            (Seg::Name(name), Type::Struct(st)) => {
                let field = st
                    .fields
                    .iter()
                    .find(|f| f.name == *name)
                    .ok_or(Error::UnknownPath)?;
                offset = offset.checked_add(field.offset).ok_or(Error::Overflow)?;
                ty = &field.ty;
            }
            (Seg::Name(name), Type::Bitpack(bp)) => {
                let field = bp
                    .fields
                    .iter()
                    .find(|f| f.name == *name)
                    .ok_or(Error::UnknownPath)?;
                bit_field = Some(field);
            }
            (Seg::Index(index), Type::Array(arr)) => {
                if *index >= arr.count {
                    return Err(Error::OutOfBounds);
                }
                let stride = arr.elem.inline_size();
                let delta = index.checked_mul(stride).ok_or(Error::Overflow)?;
                offset = offset.checked_add(delta).ok_or(Error::Overflow)?;
                ty = arr.elem.as_ref();
            }
            (Seg::Index(index), Type::Slice(sl)) => {
                let payload_off = read_u64(bytes, offset)?;
                let count = read_u64(bytes, offset.checked_add(8).ok_or(Error::Overflow)?)?;
                if *index >= count {
                    return Err(Error::OutOfBounds);
                }
                let stride = sl.elem.inline_size();
                let delta = index.checked_mul(stride).ok_or(Error::Overflow)?;
                offset = payload_off.checked_add(delta).ok_or(Error::Overflow)?;
                ty = sl.elem.as_ref();
            }
            _ => return Err(Error::UnknownPath),
        }
    }
    Ok(Cursor {
        ty,
        offset,
        bit_field,
    })
}

fn read_bit_field(ty: &Type, field: &BitField, offset: u64, bytes: &[u8]) -> Result<Value> {
    let Type::Bitpack(bp) = ty else {
        return Err(Error::TypeMismatch);
    };
    check_align(offset, bp.align)?;
    match bp.container {
        Some(container) => {
            let cell = load_container(container, offset, bytes)?;
            let value = extract_bits(cell, field.bit_offset, field.width, container)?;
            Ok(Value::U64(value))
        }
        None => {
            let packed = bitpack_span_bytes(bytes, offset, bp.span)?;
            let value = extract_span_bits(packed, field.bit_offset, field.width)?;
            Ok(Value::U64(value))
        }
    }
}

fn write_bit_field(
    ty: &Type,
    field: &BitField,
    offset: u64,
    bytes: &mut [u8],
    value: &Value,
) -> Result<()> {
    let Type::Bitpack(bp) = ty else {
        return Err(Error::TypeMismatch);
    };
    let Value::U64(v) = value else {
        return Err(Error::TypeMismatch);
    };
    check_align(offset, bp.align)?;
    match bp.container {
        Some(container) => {
            let cell = load_container(container, offset, bytes)?;
            let cell = insert_bits(cell, field.bit_offset, field.width, *v, container)?;
            store_container(container, offset, bytes, cell)
        }
        None => {
            let packed = bitpack_span_bytes_mut(bytes, offset, bp.span)?;
            insert_span_bits(packed, field.bit_offset, field.width, *v)
        }
    }
}

fn bit_mask(width: u64) -> Result<u64> {
    match width {
        0 => Err(Error::Overflow),
        1..=63 => Ok((1u64 << width) - 1),
        64 => Ok(u64::MAX),
        _ => Err(Error::Overflow),
    }
}

fn check_field_span(container: Scalar, bit_offset: u64, width: u64) -> Result<()> {
    let cap = container.size().checked_mul(8).ok_or(Error::Overflow)?;
    let end = bit_offset.checked_add(width).ok_or(Error::Overflow)?;
    if end > cap {
        Err(Error::Overflow)
    } else {
        Ok(())
    }
}

fn check_span_bit_range(span: u64, bit_offset: u64, width: u64) -> Result<()> {
    let cap = span.checked_mul(8).ok_or(Error::Overflow)?;
    let end = bit_offset.checked_add(width).ok_or(Error::Overflow)?;
    if end > cap {
        Err(Error::Overflow)
    } else {
        Ok(())
    }
}

fn bitpack_span_bytes(bytes: &[u8], offset: u64, size: u64) -> Result<&[u8]> {
    let start = span(bytes, offset, size)?;
    let len = usize::try_from(size).map_err(|_| Error::OutOfBounds)?;
    Ok(&bytes[start..start + len])
}

fn bitpack_span_bytes_mut(bytes: &mut [u8], offset: u64, size: u64) -> Result<&mut [u8]> {
    let start = span(bytes, offset, size)?;
    let len = usize::try_from(size).map_err(|_| Error::OutOfBounds)?;
    Ok(&mut bytes[start..start + len])
}

/// Memory lsb0: bit `k` is the LSB of byte `k/8`. Fields may cross bytes.
fn extract_span_bits(packed: &[u8], bit_offset: u64, width: u64) -> Result<u64> {
    let span_len = u64::try_from(packed.len()).map_err(|_| Error::Overflow)?;
    check_span_bit_range(span_len, bit_offset, width)?;
    let mut value = 0u64;
    let mut remaining = width;
    let mut src_bit = bit_offset;
    let mut dst_bit = 0u64;
    while remaining > 0 {
        let byte_i = usize::try_from(src_bit / 8).map_err(|_| Error::Overflow)?;
        let bit_in_byte = src_bit % 8;
        let take = remaining.min(8 - bit_in_byte);
        let chunk = (u64::from(packed[byte_i]) >> bit_in_byte) & bit_mask(take)?;
        value |= chunk << dst_bit;
        src_bit += take;
        dst_bit += take;
        remaining -= take;
    }
    Ok(value)
}

fn insert_span_bits(packed: &mut [u8], bit_offset: u64, width: u64, value: u64) -> Result<()> {
    let span_len = u64::try_from(packed.len()).map_err(|_| Error::Overflow)?;
    check_span_bit_range(span_len, bit_offset, width)?;
    let mask = bit_mask(width)?;
    if value & !mask != 0 {
        return Err(Error::TypeMismatch);
    }
    let mut remaining = width;
    let mut src_bit = bit_offset;
    let mut val_bit = 0u64;
    while remaining > 0 {
        let byte_i = usize::try_from(src_bit / 8).map_err(|_| Error::Overflow)?;
        let bit_in_byte = src_bit % 8;
        let take = remaining.min(8 - bit_in_byte);
        let chunk_mask = bit_mask(take)?;
        let chunk = (value >> val_bit) & chunk_mask;
        let shift = bit_in_byte as u32;
        let byte_mask = (chunk_mask as u8).wrapping_shl(shift);
        let inserted = (chunk as u8).wrapping_shl(shift);
        packed[byte_i] = (packed[byte_i] & !byte_mask) | inserted;
        src_bit += take;
        val_bit += take;
        remaining -= take;
    }
    Ok(())
}

fn extract_bits(cell: u64, bit_offset: u64, width: u64, container: Scalar) -> Result<u64> {
    check_field_span(container, bit_offset, width)?;
    if bit_offset >= 64 {
        return Err(Error::Overflow);
    }
    let mask = bit_mask(width)?;
    Ok((cell >> bit_offset) & mask)
}

fn insert_bits(
    cell: u64,
    bit_offset: u64,
    width: u64,
    value: u64,
    container: Scalar,
) -> Result<u64> {
    check_field_span(container, bit_offset, width)?;
    if bit_offset >= 64 {
        return Err(Error::Overflow);
    }
    let mask = bit_mask(width)?;
    if value & !mask != 0 {
        return Err(Error::TypeMismatch);
    }
    if width > 64 - bit_offset {
        return Err(Error::Overflow);
    }
    let shifted_mask = mask << bit_offset;
    let shifted_val = value << bit_offset;
    Ok((cell & !shifted_mask) | shifted_val)
}

fn load_container(container: Scalar, offset: u64, bytes: &[u8]) -> Result<u64> {
    match read_scalar(container, offset, bytes)? {
        Value::U8(v) => Ok(u64::from(v)),
        Value::U16(v) => Ok(u64::from(v)),
        Value::U32(v) => Ok(u64::from(v)),
        Value::U64(v) => Ok(v),
        _ => Err(Error::TypeMismatch),
    }
}

fn store_container(container: Scalar, offset: u64, bytes: &mut [u8], cell: u64) -> Result<()> {
    let value = match container {
        Scalar::U8 => Value::U8(cell as u8),
        Scalar::U16 => Value::U16(cell as u16),
        Scalar::U32 => Value::U32(cell as u32),
        Scalar::U64 => Value::U64(cell),
        _ => return Err(Error::TypeMismatch),
    };
    write_scalar(container, offset, bytes, &value)
}

fn read_value(ty: &Type, offset: u64, bytes: &[u8]) -> Result<Value> {
    check_align(offset, ty.align())?;
    match ty {
        Type::Scalar(s) => read_scalar(*s, offset, bytes),
        Type::Array(arr) => {
            let mut elems = Vec::with_capacity(usize_count(arr.count)?);
            let stride = arr.elem.inline_size();
            let mut pos = offset;
            for _ in 0..arr.count {
                elems.push(read_value(arr.elem.as_ref(), pos, bytes)?);
                pos = pos.checked_add(stride).ok_or(Error::Overflow)?;
            }
            Ok(Value::Array(elems))
        }
        Type::Slice(sl) => {
            let payload_off = read_u64(bytes, offset)?;
            let count = read_u64(bytes, offset.checked_add(8).ok_or(Error::Overflow)?)?;
            let stride = sl.elem.inline_size();
            let mut elems = Vec::with_capacity(usize_count(count)?);
            let mut pos = payload_off;
            for _ in 0..count {
                elems.push(read_value(sl.elem.as_ref(), pos, bytes)?);
                pos = pos.checked_add(stride).ok_or(Error::Overflow)?;
            }
            Ok(Value::Slice {
                offset: payload_off,
                elems,
            })
        }
        Type::Struct(st) => {
            let mut fields = Vec::with_capacity(st.fields.len());
            for field in &st.fields {
                let pos = offset.checked_add(field.offset).ok_or(Error::Overflow)?;
                fields.push((field.name.clone(), read_value(&field.ty, pos, bytes)?));
            }
            Ok(Value::Struct(fields))
        }
        Type::Link => Ok(Value::U64(read_u64(bytes, offset)?)),
        Type::Columnar(_) | Type::Bitpack(_) => Err(Error::TypeMismatch),
    }
}

fn write_value(ty: &Type, offset: u64, bytes: &mut [u8], value: &Value) -> Result<()> {
    check_align(offset, ty.align())?;
    match (ty, value) {
        (Type::Scalar(s), v) => write_scalar(*s, offset, bytes, v),
        (Type::Array(arr), Value::Array(elems)) => {
            if elems.len() as u64 != arr.count {
                return Err(Error::TypeMismatch);
            }
            let stride = arr.elem.inline_size();
            let mut pos = offset;
            for elem in elems {
                write_value(arr.elem.as_ref(), pos, bytes, elem)?;
                pos = pos.checked_add(stride).ok_or(Error::Overflow)?;
            }
            Ok(())
        }
        (
            Type::Slice(sl),
            Value::Slice {
                offset: payload_off,
                elems,
            },
        ) => {
            let count = u64_count(elems.len())?;
            let stride = sl.elem.inline_size();
            check_align(*payload_off, sl.elem.align())?;
            let mut pos = *payload_off;
            for elem in elems {
                write_value(sl.elem.as_ref(), pos, bytes, elem)?;
                pos = pos.checked_add(stride).ok_or(Error::Overflow)?;
            }
            write_u64(bytes, offset, *payload_off)?;
            write_u64(bytes, offset.checked_add(8).ok_or(Error::Overflow)?, count)?;
            Ok(())
        }
        (Type::Struct(st), Value::Struct(pairs)) => {
            if pairs.len() != st.fields.len() {
                return Err(Error::TypeMismatch);
            }
            for (field, (name, val)) in st.fields.iter().zip(pairs.iter()) {
                if field.name != *name {
                    return Err(Error::TypeMismatch);
                }
                let pos = offset.checked_add(field.offset).ok_or(Error::Overflow)?;
                write_value(&field.ty, pos, bytes, val)?;
            }
            Ok(())
        }
        (Type::Link, Value::U64(v)) => write_u64(bytes, offset, *v),
        _ => Err(Error::TypeMismatch),
    }
}

fn read_scalar(s: Scalar, offset: u64, bytes: &[u8]) -> Result<Value> {
    let start = span(bytes, offset, s.size())?;
    let chunk = &bytes[start..start + s.size() as usize];
    Ok(match s {
        Scalar::I8 => Value::I8(chunk[0] as i8),
        Scalar::U8 => Value::U8(chunk[0]),
        Scalar::I16 => Value::I16(i16::from_le_bytes(copy2(chunk)?)),
        Scalar::U16 => Value::U16(u16::from_le_bytes(copy2(chunk)?)),
        Scalar::I32 => Value::I32(i32::from_le_bytes(copy4(chunk)?)),
        Scalar::U32 => Value::U32(u32::from_le_bytes(copy4(chunk)?)),
        Scalar::I64 => Value::I64(i64::from_le_bytes(copy8(chunk)?)),
        Scalar::U64 => Value::U64(u64::from_le_bytes(copy8(chunk)?)),
        Scalar::F32 => Value::F32(f32::from_le_bytes(copy4(chunk)?)),
        Scalar::F64 => Value::F64(f64::from_le_bytes(copy8(chunk)?)),
    })
}

fn write_scalar(s: Scalar, offset: u64, bytes: &mut [u8], value: &Value) -> Result<()> {
    let start = span(bytes, offset, s.size())?;
    let dest = &mut bytes[start..start + s.size() as usize];
    match (s, value) {
        (Scalar::I8, Value::I8(v)) => dest[0] = *v as u8,
        (Scalar::U8, Value::U8(v)) => dest[0] = *v,
        (Scalar::I16, Value::I16(v)) => dest.copy_from_slice(&v.to_le_bytes()),
        (Scalar::U16, Value::U16(v)) => dest.copy_from_slice(&v.to_le_bytes()),
        (Scalar::I32, Value::I32(v)) => dest.copy_from_slice(&v.to_le_bytes()),
        (Scalar::U32, Value::U32(v)) => dest.copy_from_slice(&v.to_le_bytes()),
        (Scalar::I64, Value::I64(v)) => dest.copy_from_slice(&v.to_le_bytes()),
        (Scalar::U64, Value::U64(v)) => dest.copy_from_slice(&v.to_le_bytes()),
        (Scalar::F32, Value::F32(v)) => dest.copy_from_slice(&v.to_le_bytes()),
        (Scalar::F64, Value::F64(v)) => dest.copy_from_slice(&v.to_le_bytes()),
        _ => return Err(Error::TypeMismatch),
    }
    Ok(())
}

fn read_u64(bytes: &[u8], offset: u64) -> Result<u64> {
    check_align(offset, 8)?;
    let start = span(bytes, offset, 8)?;
    Ok(u64::from_le_bytes(copy8(&bytes[start..start + 8])?))
}

fn write_u64(bytes: &mut [u8], offset: u64, value: u64) -> Result<()> {
    check_align(offset, 8)?;
    let start = span(bytes, offset, 8)?;
    bytes[start..start + 8].copy_from_slice(&value.to_le_bytes());
    Ok(())
}

fn span(bytes: &[u8], offset: u64, size: u64) -> Result<usize> {
    let start = usize::try_from(offset).map_err(|_| Error::OutOfBounds)?;
    let len = usize::try_from(size).map_err(|_| Error::OutOfBounds)?;
    let end = start.checked_add(len).ok_or(Error::Overflow)?;
    if end > bytes.len() {
        Err(Error::OutOfBounds)
    } else {
        Ok(start)
    }
}

fn check_align(offset: u64, align: u64) -> Result<()> {
    if align == 0 || !align.is_power_of_two() {
        return Err(Error::BadAlign);
    }
    if !offset.is_multiple_of(align) {
        Err(Error::BadAlign)
    } else {
        Ok(())
    }
}

fn usize_count(count: u64) -> Result<usize> {
    usize::try_from(count).map_err(|_| Error::Overflow)
}

fn u64_count(count: usize) -> Result<u64> {
    u64::try_from(count).map_err(|_| Error::Overflow)
}

fn copy2(chunk: &[u8]) -> Result<[u8; 2]> {
    chunk.try_into().map_err(|_| Error::OutOfBounds)
}

fn copy4(chunk: &[u8]) -> Result<[u8; 4]> {
    chunk.try_into().map_err(|_| Error::OutOfBounds)
}

fn copy8(chunk: &[u8]) -> Result<[u8; 8]> {
    chunk.try_into().map_err(|_| Error::OutOfBounds)
}
