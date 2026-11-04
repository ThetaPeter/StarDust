use std::ptr;

#[no_mangle]
pub unsafe extern "C" fn touch(p: *mut u8, n: usize) {
    if n > 0 && !p.is_null() {
        *p = 1;
    }
}

#[no_mangle]
pub unsafe extern "C" fn set_minor(db: *mut u8, db_len: usize, value: *mut u8, value_len: usize) {
    if db.is_null() || db_len < 8 {
        return;
    }
    if value_len > 0 && value.is_null() {
        return;
    }
    let n = value_len.min(db_len.saturating_sub(8)).min(32);
    if n > 0 {
        ptr::copy_nonoverlapping(value, db.add(8), n);
    }
    let len_bytes = (n as u64).to_le_bytes();
    ptr::copy_nonoverlapping(len_bytes.as_ptr(), db, 8);
}

#[no_mangle]
pub unsafe extern "C" fn get_minor(db: *mut u8, db_len: usize, out: *mut u8, out_len: usize) {
    if db.is_null() || db_len < 8 || out.is_null() {
        return;
    }
    let mut len_bytes = [0u8; 8];
    ptr::copy_nonoverlapping(db, len_bytes.as_mut_ptr(), 8);
    let stored = u64::from_le_bytes(len_bytes);
    let stored = usize::try_from(stored).unwrap_or(0);
    let avail = db_len.saturating_sub(8);
    let n = stored.min(out_len).min(avail);
    if n == 0 {
        return;
    }
    ptr::copy_nonoverlapping(db.add(8), out, n);
}

#[no_mangle]
pub unsafe extern "C" fn print_minor(db: *mut u8, db_len: usize) {
    if db_len > 0 && !db.is_null() {
        *db.add(db_len - 1) = 1;
    }
}

#[no_mangle]
pub unsafe extern "C" fn record_span(
    instance: *mut u8,
    instance_len: usize,
    _value: *mut u8,
    _value_len: usize,
) {
    if instance.is_null() || instance_len < 8 {
        return;
    }
    let bytes = (instance_len as u64).to_le_bytes();
    ptr::copy_nonoverlapping(bytes.as_ptr(), instance, 8);
}
