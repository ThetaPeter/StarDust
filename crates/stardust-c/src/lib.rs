/// Absolute filesystem path of the compiled in-tree `process` shared library.
pub fn process_lib_path() -> &'static str {
    env!("STARDUST_C_LIB")
}
