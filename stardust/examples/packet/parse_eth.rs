/// Same checksum as `parse.c`: skip the little-endian u32 at offset 4.
#[no_mangle]
pub unsafe extern "C" fn parse_eth(p: *mut u8, n: usize) {
    if p.is_null() || n < 8 {
        return;
    }
    let mut sum: u32 = 0;
    for i in 0..n {
        if (4..8).contains(&i) {
            continue;
        }
        sum = sum.wrapping_add(unsafe { *p.add(i) } as u32);
    }
    let bytes = sum.to_le_bytes();
    unsafe {
        *p.add(4) = bytes[0];
        *p.add(5) = bytes[1];
        *p.add(6) = bytes[2];
        *p.add(7) = bytes[3];
    }
}
