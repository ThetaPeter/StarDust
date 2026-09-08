/// Absolute filesystem path of the compiled in-tree Haskell `mutate` shared library.
pub fn mutate_lib_path() -> &'static str {
    ensure_runtime_search_path();
    env!("STARDUST_HS_LIB")
}

fn ensure_runtime_search_path() {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let lib = std::path::Path::new(env!("STARDUST_HS_LIB"));
        if let Some(dir) = lib.parent() {
            prepend_dll_search_path(dir);
        }
    });
}

fn prepend_dll_search_path(dir: &std::path::Path) {
    #[cfg(windows)]
    {
        let extra = dir.to_string_lossy().into_owned();
        match std::env::var("PATH") {
            Ok(path) => {
                if !path.split(';').any(|p| p.eq_ignore_ascii_case(&extra)) {
                    // PATH must be updated before Library::load on Windows.
                    unsafe { std::env::set_var("PATH", extra + ";" + &path) };
                }
            }
            Err(_) => unsafe { std::env::set_var("PATH", extra) },
        }
    }
    #[cfg(not(windows))]
    {
        let _ = dir;
    }
}
