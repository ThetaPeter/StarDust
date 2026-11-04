/// Absolute filesystem path of the compiled in-tree `project` shared library.
pub fn project_lib_path() -> &'static str {
    env!("STARDUST_RS_LIB")
}
