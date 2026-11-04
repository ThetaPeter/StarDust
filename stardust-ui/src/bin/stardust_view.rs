#![windows_subsystem = "windows"]
//! Native WebView2 window for StarDust UI (not Edge `--app=`).
//! HTTP stays in `fs-viz` (GNU windows-sys in the same PE AVs). This process
//! only hosts WebView2. Taskbar pin: StarDust UI is the work-area cover.

#[cfg(windows)]
#[path = "../webview_host.rs"]
mod webview_host;

fn main() {
    #[cfg(windows)]
    {
        let mut url = String::new();
        let mut desktop = false;
        let mut profile = fs_viz::shell::config_dir().join("webview-profile");
        let mut args = std::env::args().skip(1);
        while let Some(a) = args.next() {
            if a == "--desktop" {
                desktop = true;
            } else if a == "--profile" {
                if let Some(p) = args.next() {
                    profile = std::path::PathBuf::from(p);
                }
            } else if !a.starts_with('-') {
                url = a;
            }
        }
        if url.is_empty() {
            desktop = true;
        }
        let mode = fs_viz::shell::load_mode().mode;
        let (url, desktop) = fs_viz::shell::native_view_launch(&url, desktop, mode);
        let _ = std::fs::create_dir_all(&profile);
        if let Err(_) = webview_host::run(&url, &profile, desktop) {
            std::process::exit(1);
        }
    }
    #[cfg(not(windows))]
    {
        std::process::exit(1);
    }
}
