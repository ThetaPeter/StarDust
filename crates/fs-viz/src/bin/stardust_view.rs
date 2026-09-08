#![windows_subsystem = "windows"]
//! Native WebView2 window for StarDust UI (not Edge `--app=`).
//! Waits for `GET /api/healthz` 200 before Navigate; starts `fs-viz --desktop` if StarDust
//! UI is on and HTTP is down; retries Navigate on a timer (no COM NavigationCompleted sink).

#[cfg(windows)]
#[path = "../webview_host.rs"]
mod webview_host;

fn main() {
    #[cfg(windows)]
    {
        fs_viz::shell::enable_dpi_aware();
        let mut url = String::from("http://127.0.0.1:6006/?desktop=1");
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
        if url.contains("desktop=1") {
            desktop = true;
        }
        if let Err(e) = webview_host::run(&url, &profile, desktop) {
            eprintln!("stardust-view: {e}");
            std::process::exit(1);
        }
    }
    #[cfg(not(windows))]
    {
        eprintln!("stardust-view is Windows-only");
        std::process::exit(1);
    }
}
