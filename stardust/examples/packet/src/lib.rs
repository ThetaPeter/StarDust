/// Compiled `parse.c` (`parse_frame`, `seal_db`).
pub fn c_lib_path() -> &'static str {
    env!("STARDUST_PACKET_C_LIB")
}

/// Compiled `parse_eth.rs` (`parse_eth`).
pub fn rs_lib_path() -> &'static str {
    env!("STARDUST_PACKET_RS_LIB")
}
