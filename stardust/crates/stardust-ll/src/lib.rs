/// Absolute filesystem path of the compiled in-tree LLVM `mutate` shared library.
pub fn mutate_lib_path() -> &'static str {
    env!("STARDUST_LL_LIB")
}
