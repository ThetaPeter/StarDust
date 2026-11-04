//! Windows Explorer/Desktop override: mode, trampoline, guard, HKCU backup.
//! Not a language-wave module. Cosmos UI is the product surface.

use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::Mutex;
#[cfg(windows)]
use std::sync::OnceLock;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const BIND: &str = "127.0.0.1:6006";
pub const FAILSAFE_HOTKEY: &str = "Ctrl+Shift+F12";

pub fn enable_dpi_aware() {
    // GNU delay-load of windows-sys / extra kernel32 thunks currently AVs (BEX64).
    // HTTP and file IO via libstd still work. Restore DPI once that IAT is fixed.
}
const OPEN_TIMEOUT: Duration = Duration::from_secs(3);
const HEARTBEAT_STALE: Duration = Duration::from_secs(5);
const UI_PULSE_STALE: Duration = Duration::from_secs(8);
const ARM_GRACE_SECS: u64 = 20;
/// Process-local login window (not `ui-mode.json` `armed_at`, which is stale after reboot).
pub const BOOT_GRACE_SECS: u64 = 60;
/// After bind, wait this long for `GET /api/healthz` 200 before opening the view.
pub const DESKTOP_HTTP_WAIT_SECS: u64 = 5;
/// Matches host `CARGO_TIMEOUT` (600s) plus ~60s for `--desktop --replace` after `:6006` is free.
const REBUILD_HOLD_SECS: u64 = 660;
const HOST_HEADLESS_ARG: &str = "--headless";
const HOST_DESKTOP_ARG: &str = "--desktop";
/// `CREATE_NO_WINDOW` — no taskbar console for helper `Command`s.
pub const CREATE_NO_WINDOW: u32 = 0x08000000;

/// Hide the helper console and do not inherit std handles.
///
/// `--desktop` / `--open` / `--headless` call `FreeConsole`, so inherited
/// stdin/stdout/stderr are invalid. `CreateProcess` then fails with
/// `ERROR_INVALID_HANDLE` (os error 6, 控制代碼無效). Callers that capture
/// output must set `Stdio::piped` **after** this.
pub fn windows_no_window(cmd: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    {
        let _ = CREATE_NO_WINDOW;
    }
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
}

fn spawn_no_window(cmd: &mut Command) -> io::Result<std::process::Child> {
    windows_no_window(cmd);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
}

/// Detach this process from its console (HKCU Run `fs-viz --desktop` / `--open` / `--headless`).
pub fn detach_console() {
    #[cfg(windows)]
    unsafe {
        let _ = windows_sys::Win32::System::Console::FreeConsole();
    }
}

const BURST_WINDOW_SECS: u64 = 5;
const BURST_CAP: u32 = 20;

/// Unix seconds when `guard_main` started. `0` = not the guard process (HTTP/probe from fs-viz).
static GUARD_BOOT_AT: AtomicU64 = AtomicU64::new(0);
/// Guard/host: restore Snap once per Windows-UI stretch so we do not fight Settings.
static SNAP_WINDOWS_APPLIED: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UiMode {
    Stardust,
    Windows,
}

impl UiMode {
    pub fn as_str(self) -> &'static str {
        match self {
            UiMode::Stardust => "stardust",
            UiMode::Windows => "windows",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "stardust" => Some(UiMode::Stardust),
            "windows" => Some(UiMode::Windows),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UiModeFile {
    pub mode: UiMode,
    #[serde(default)]
    pub replace_armed: bool,
    #[serde(default)]
    pub hide_desktop_icons: bool,
    #[serde(default)]
    pub restart_used: bool,
    #[serde(default)]
    pub last_failover: String,
    #[serde(default)]
    pub armed_at: u64,
    /// Unix seconds; `0` = no rebuild hold. Covers cargo + relaunch HTTP gap.
    #[serde(default)]
    pub rebuild_hold_until: u64,
    /// True when the three snap fields are the user's pre-StarDust values.
    #[serde(default)]
    pub snap_pref_saved: bool,
    #[serde(default)]
    pub snap_win_arranging: bool,
    #[serde(default)]
    pub snap_sizing: bool,
    #[serde(default)]
    pub snap_dock_moving: bool,
}

impl Default for UiModeFile {
    fn default() -> Self {
        Self {
            mode: UiMode::Windows,
            replace_armed: false,
            hide_desktop_icons: false,
            restart_used: false,
            last_failover: String::new(),
            armed_at: 0,
            rebuild_hold_until: 0,
            snap_pref_saved: false,
            snap_win_arranging: false,
            snap_sizing: false,
            snap_dock_moving: false,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HkcuBackup {
    pub directory_open: Option<String>,
    pub drive_open: Option<String>,
    pub hide_icons: Option<u32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HealthFile {
    pub color: String,
    pub reason: String,
    pub server_ok: bool,
    pub heartbeat_ok: bool,
    pub ui_pulse_ok: bool,
    pub registry_ok: bool,
    pub backup_ok: bool,
    pub ts: u64,
}

pub fn config_dir() -> PathBuf {
    #[cfg(windows)]
    {
        if let Some(appdata) = std::env::var_os("APPDATA") {
            return PathBuf::from(appdata).join("StarDust");
        }
    }
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
        return PathBuf::from(xdg).join("stardust");
    }
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"));
    if let Some(home) = home {
        return PathBuf::from(home).join(".config").join("stardust");
    }
    PathBuf::from("stardust-config")
}

pub fn ensure_config_dir() -> io::Result<PathBuf> {
    let d = config_dir();
    fs::create_dir_all(&d)?;
    Ok(d)
}

pub fn mode_path() -> PathBuf {
    config_dir().join("ui-mode.json")
}

pub fn health_path() -> PathBuf {
    config_dir().join("ui-health.json")
}

pub fn backup_path() -> PathBuf {
    config_dir().join("hkcu-folder-backup.json")
}

pub fn heartbeat_path() -> PathBuf {
    config_dir().join("ui-heartbeat.json")
}

pub fn ui_pulse_path() -> PathBuf {
    config_dir().join("ui-pulse.json")
}

pub fn guard_log_path() -> PathBuf {
    config_dir().join("ui-guard.log")
}

fn burst_path() -> PathBuf {
    config_dir().join("open-burst.json")
}

fn read_mode_file() -> Option<UiModeFile> {
    let text = fs::read_to_string(mode_path()).ok()?;
    serde_json::from_str(&text).ok()
}

pub fn load_mode() -> UiModeFile {
    read_mode_file().unwrap_or_default()
}

fn ui_run_installed() -> bool {
    #[cfg(windows)]
    {
        hkcu_get(
            r"Software\Microsoft\Windows\CurrentVersion\Run",
            Some("StarDustUI"),
        )
        .is_some()
    }
    #[cfg(not(windows))]
    {
        false
    }
}

pub fn save_mode(file: &UiModeFile) -> io::Result<()> {
    ensure_config_dir()?;
    let tmp = mode_path().with_extension("json.tmp");
    fs::write(
        &tmp,
        serde_json::to_vec_pretty(file).unwrap_or_else(|_| b"{}".to_vec()),
    )?;
    fs::rename(&tmp, mode_path())?;
    Ok(())
}

pub fn write_health(h: &HealthFile) {
    let _ = ensure_config_dir();
    let _ = fs::write(
        health_path(),
        serde_json::to_vec(h).unwrap_or_else(|_| b"{}".to_vec()),
    );
}

pub fn touch_heartbeat() {
    let _ = ensure_config_dir();
    let _ = fs::write(heartbeat_path(), json!({ "ts": unix_now() }).to_string());
}

pub fn touch_ui_pulse() {
    let _ = ensure_config_dir();
    let _ = fs::write(ui_pulse_path(), json!({ "ts": unix_now() }).to_string());
}

pub fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn file_age_secs(path: &Path, max: Duration) -> bool {
    let Ok(meta) = fs::metadata(path) else {
        return false;
    };
    let Ok(modified) = meta.modified() else {
        return false;
    };
    SystemTime::now()
        .duration_since(modified)
        .map(|d| d <= max)
        .unwrap_or(false)
}

#[cfg(windows)]
fn known_folder(id: &windows_sys::core::GUID) -> Option<PathBuf> {
    use windows_sys::Win32::System::Com::CoTaskMemFree;
    use windows_sys::Win32::UI::Shell::SHGetKnownFolderPath;
    let mut p: windows_sys::core::PWSTR = std::ptr::null_mut();
    let hr = unsafe { SHGetKnownFolderPath(id, 0, std::ptr::null_mut(), &mut p) };
    if hr != 0 || p.is_null() {
        return None;
    }
    let path = unsafe {
        let mut len = 0usize;
        while *p.add(len) != 0 {
            len += 1;
        }
        let s = String::from_utf16_lossy(std::slice::from_raw_parts(p, len));
        CoTaskMemFree(p.cast());
        PathBuf::from(s)
    };
    if path.is_dir() {
        Some(path)
    } else {
        None
    }
}

#[cfg(windows)]
fn known_desktop() -> Option<PathBuf> {
    use windows_sys::Win32::UI::Shell::FOLDERID_Desktop;
    known_folder(&FOLDERID_Desktop)
}

/// Common Desktop (`%PUBLIC%\Desktop`). Explorer merges this into the user Desktop view.
pub fn public_desktop_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::Shell::FOLDERID_PublicDesktop;
        if let Some(p) = known_folder(&FOLDERID_PublicDesktop) {
            return Some(p);
        }
        if let Ok(public) = std::env::var("PUBLIC") {
            let d = PathBuf::from(public).join("Desktop");
            if d.is_dir() {
                return Some(d);
            }
        }
    }
    None
}

pub fn desktop_dir() -> PathBuf {
    #[cfg(windows)]
    {
        if let Some(p) = known_desktop() {
            return p;
        }
        if let Ok(home) = std::env::var("USERPROFILE") {
            let od = PathBuf::from(&home).join("OneDrive").join("Desktop");
            if od.is_dir() {
                return od;
            }
            return PathBuf::from(home).join("Desktop");
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home).join("Desktop");
    }
    PathBuf::from(".")
}

pub fn session_root(repo: &Path) -> PathBuf {
    let mode = load_mode();
    if mode.mode == UiMode::Stardust {
        let d = desktop_dir();
        if d.is_dir() {
            return d;
        }
    }
    repo.to_path_buf()
}

pub fn list_drives() -> Vec<Value> {
    let mut out = Vec::new();
    #[cfg(windows)]
    {
        for c in b'A'..=b'Z' {
            let p = format!("{}:\\", c as char);
            let path = PathBuf::from(&p);
            if path.is_dir() {
                let (free, total) = volume_space(&path);
                out.push(json!({
                    "name": format!("{}:", c as char),
                    "path": p,
                    "kind": "dir",
                    "free": free,
                    "total": total,
                }));
            }
        }
    }
    #[cfg(not(windows))]
    {
        out.push(json!({
            "name": "/",
            "path": "/",
            "kind": "dir",
            "free": 0,
            "total": 0,
        }));
    }
    out
}

#[cfg(windows)]
fn volume_space(path: &Path) -> (u64, u64) {
    use std::os::windows::ffi::OsStrExt;
    let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    if wide.last() != Some(&0) {
        wide.push(0);
    }
    let mut free = 0u64;
    let mut total = 0u64;
    unsafe {
        let ok = windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW(
            wide.as_ptr(),
            std::ptr::null_mut(),
            &mut total,
            &mut free,
        );
        if ok == 0 {
            return (0, 0);
        }
    }
    (free, total)
}

#[cfg(not(windows))]
fn volume_space(_path: &Path) -> (u64, u64) {
    (0, 0)
}

pub fn sibling_exe(name: &str) -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            #[cfg(windows)]
            {
                return dir.join(format!("{name}.exe"));
            }
            #[cfg(not(windows))]
            {
                return dir.join(name);
            }
        }
    }
    PathBuf::from(name)
}

pub fn http_ok() -> bool {
    let addr = match BIND.parse() {
        Ok(a) => a,
        Err(_) => return false,
    };
    let Ok(mut s) = std::net::TcpStream::connect_timeout(&addr, Duration::from_millis(400)) else {
        return false;
    };
    let _ = s.set_read_timeout(Some(Duration::from_millis(400)));
    let _ = s.set_write_timeout(Some(Duration::from_millis(400)));
    let req = b"GET /api/healthz HTTP/1.0\r\nHost: 127.0.0.1:6006\r\nConnection: close\r\n\r\n";
    if s.write_all(req).is_err() {
        return false;
    }
    let mut buf = [0u8; 96];
    let n = s.read(&mut buf).unwrap_or(0);
    let text = String::from_utf8_lossy(&buf[..n]);
    text.contains(" 200 ")
}

pub fn wait_http(timeout: Duration) -> bool {
    let start = SystemTime::now();
    loop {
        if http_ok() {
            return true;
        }
        if start.elapsed().unwrap_or(timeout) >= timeout {
            return false;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Dual `--desktop` at login: never WM_CLOSE a live `StarDustHost`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DesktopCoverOpenPlan {
    ReuseCover,
    SpawnView { close_legacy_chrome: bool },
}

fn desktop_cover_open_plan(_http_up: bool, host_exists: bool) -> DesktopCoverOpenPlan {
    if host_exists {
        DesktopCoverOpenPlan::ReuseCover
    } else {
        DesktopCoverOpenPlan::SpawnView {
            close_legacy_chrome: true,
        }
    }
}

/// View Navigate policy: never hit Edge ERR_CONNECTION_REFUSED as the first load.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HttpReadyPlan {
    NavigateNow,
    StartHostThenWait,
}

fn http_ready_plan(http_up: bool, _stardust: bool) -> HttpReadyPlan {
    if http_up {
        HttpReadyPlan::NavigateNow
    } else {
        HttpReadyPlan::StartHostThenWait
    }
}

fn is_desktop_cover_url(url: &str) -> bool {
    url.contains("desktop=1")
}

/// Taskbar `stardust-view` / leftover `--desktop` / `?desktop=1` must follow saved UI mode:
/// Windows UI is a captioned window on `/`, never the work-area cover.
pub fn native_view_launch(url: &str, desktop_flag: bool, mode: UiMode) -> (String, bool) {
    if mode == UiMode::Stardust {
        if desktop_flag || is_desktop_cover_url(url) || url.is_empty() {
            (desktop_url(), true)
        } else {
            (url.to_string(), false)
        }
    } else if url.is_empty() || is_desktop_cover_url(url) {
        (format!("http://{BIND}/"), false)
    } else {
        (url.to_string(), false)
    }
}

fn stardust_host_exists() -> bool {
    #[cfg(windows)]
    {
        desktop_cover::native_host_exists()
    }
    #[cfg(not(windows))]
    {
        false
    }
}

/// True when a visible `StarDustHost` exists (cover or floated window).
pub fn wait_stardust_hwnd(timeout: Duration) -> bool {
    #[cfg(windows)]
    {
        let start = SystemTime::now();
        loop {
            if stardust_host_exists() {
                return true;
            }
            if start.elapsed().unwrap_or(timeout) >= timeout {
                return false;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    #[cfg(not(windows))]
    {
        let _ = timeout;
        false
    }
}

/// If `:6006` is down, start the sibling host. StarDust UI uses `--desktop` (cover);
/// Windows UI uses `--headless` so a taskbar pin can still load a captioned window.
pub fn start_desktop_host_if_needed() {
    match http_ready_plan(http_ok(), load_mode().mode == UiMode::Stardust) {
        HttpReadyPlan::NavigateNow => {}
        HttpReadyPlan::StartHostThenWait => {
            if load_mode().mode == UiMode::Stardust {
                let _ = start_host_desktop();
            } else {
                let _ = start_host_headless();
            }
        }
    }
}

fn append_log(line: &str) {
    let _ = ensure_config_dir();
    let path = guard_log_path();
    if let Ok(meta) = fs::metadata(&path) {
        if meta.len() > 512 * 1024 {
            let _ = fs::rename(&path, path.with_extension("log.1"));
        }
    }
    if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "{} {}", unix_now(), line);
    }
}

pub fn note_trampoline_timeout() {
    append_log("trampoline-timeout");
    let _ = fs::write(
        config_dir().join("trampoline-fail.json"),
        json!({ "ts": unix_now() }).to_string(),
    );
}

pub fn record_open_burst() -> bool {
    let _ = ensure_config_dir();
    let now = unix_now();
    let mut n = 1u32;
    let mut t0 = now;
    if let Ok(v) = fs::read_to_string(burst_path()) {
        if let Ok(j) = serde_json::from_str::<Value>(&v) {
            t0 = j.get("t").and_then(|x| x.as_u64()).unwrap_or(now);
            n = j.get("n").and_then(|x| x.as_u64()).unwrap_or(0) as u32;
            if now.saturating_sub(t0) > BURST_WINDOW_SECS {
                t0 = now;
                n = 0;
            }
            n = n.saturating_add(1);
        }
    }
    let _ = fs::write(burst_path(), json!({ "t": t0, "n": n }).to_string());
    n > BURST_CAP
}

/// ShellExecute `lpParameters`: extra flags first, then the quoted folder.
/// Cursor needs `--classic --new-window "<folder>"`; folder-then-flag is ignored.
pub fn shell_gui_params(folder: &Path, extra: &[&str]) -> String {
    let mut param = String::new();
    for flag in extra {
        if !param.is_empty() {
            param.push(' ');
        }
        param.push_str(flag);
    }
    if !param.is_empty() {
        param.push(' ');
    }
    param.push('"');
    param.push_str(&path_string(folder));
    param.push('"');
    param
}

/// Git clone root (parent of `stardust-ui/`). Cosmos file tree and Open in Cursor.
pub fn git_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")))
}

/// UI Cargo workspace (`stardust-ui/`). Rebuild and `target/debug/fs-viz`.
pub fn ui_cargo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Same as [`git_root`]. Open in Cursor uses the clone folder.
pub fn workspace_root() -> PathBuf {
    git_root()
}

fn is_cursor_gui(path: &Path) -> bool {
    path.is_file()
        && path
            .file_name()
            .and_then(|s| s.to_str())
            .map(|n| n.eq_ignore_ascii_case("Cursor.exe"))
            .unwrap_or(false)
}

fn cursor_exe_beside(path: &Path) -> Option<PathBuf> {
    let mut dir = path.parent()?;
    for _ in 0..4 {
        let cand = dir.join("Cursor.exe");
        if is_cursor_gui(&cand) {
            return Some(cand);
        }
        dir = dir.parent()?;
    }
    None
}

/// `Cursor.exe` GUI (not `cursor.cmd`). Typical `%LOCALAPPDATA%\Programs\cursor\Cursor.exe`.
pub fn find_cursor_exe() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            let typical = PathBuf::from(local)
                .join("Programs")
                .join("cursor")
                .join("Cursor.exe");
            if is_cursor_gui(&typical) {
                return Some(typical);
            }
        }
        for key in ["ProgramFiles", "ProgramFiles(x86)"] {
            if let Ok(root) = std::env::var(key) {
                let cand = PathBuf::from(root).join("cursor").join("Cursor.exe");
                if is_cursor_gui(&cand) {
                    return Some(cand);
                }
            }
        }
        for root in [r"C:\Program Files", r"C:\Program Files (x86)"] {
            let cand = PathBuf::from(root).join("cursor").join("Cursor.exe");
            if is_cursor_gui(&cand) {
                return Some(cand);
            }
        }
        for arg in ["Cursor.exe", "cursor"] {
            let mut cmd = Command::new("where");
            cmd.arg(arg);
            windows_no_window(&mut cmd);
            let Ok(out) = cmd.output() else {
                continue;
            };
            if !out.status.success() {
                continue;
            }
            for line in String::from_utf8_lossy(&out.stdout).lines() {
                let p = PathBuf::from(line.trim());
                if p.as_os_str().is_empty() {
                    continue;
                }
                if is_cursor_gui(&p) {
                    return Some(p);
                }
                if let Some(gui) = cursor_exe_beside(&p) {
                    return Some(gui);
                }
            }
        }
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                let p = dir.join("Cursor.exe");
                if is_cursor_gui(&p) {
                    return Some(p);
                }
            }
        }
        None
    }
    #[cfg(not(windows))]
    {
        let out = Command::new("which").arg("cursor").output().ok()?;
        if !out.status.success() {
            return None;
        }
        let line = String::from_utf8_lossy(&out.stdout);
        let p = PathBuf::from(line.lines().next().unwrap_or("").trim());
        p.is_file().then_some(p)
    }
}

/// `ShellExecuteW` `Cursor.exe --classic --new-window "<folder>"` (not `CREATE_NO_WINDOW`).
pub fn open_cursor_gui() -> io::Result<()> {
    let cwd = workspace_root();
    if !cwd.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "workspace root missing",
        ));
    }
    let exe = find_cursor_exe()
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "Cursor.exe not found"))?;
    shell_open_gui(&exe, &cwd, &["--classic", "--new-window"])
}

const OPEN_CURSOR_YIELD: Duration = Duration::from_secs(4);
static OPEN_CURSOR_UNTIL: Mutex<Option<Instant>> = Mutex::new(None);

/// Cover z-order yield after a view-thread Open in Cursor launch.
pub fn note_opening_cursor() {
    if let Ok(mut g) = OPEN_CURSOR_UNTIL.lock() {
        *g = Some(Instant::now() + OPEN_CURSOR_YIELD);
    }
}

/// True until the Open-in-Cursor yield deadline; clears when it passes.
pub fn opening_cursor() -> bool {
    let Ok(mut g) = OPEN_CURSOR_UNTIL.lock() else {
        return false;
    };
    match *g {
        Some(deadline) if Instant::now() < deadline => true,
        Some(_) => {
            *g = None;
            false
        }
        None => false,
    }
}

/// Registered `StarDustOpenCursor` (same atom in `fs-viz` HTTP and `stardust-view`).
pub fn wm_open_cursor() -> u32 {
    #[cfg(windows)]
    {
        static SLOT: OnceLock<u32> = OnceLock::new();
        *SLOT.get_or_init(|| {
            let name = wide("StarDustOpenCursor");
            unsafe {
                windows_sys::Win32::UI::WindowsAndMessaging::RegisterWindowMessageW(name.as_ptr())
            }
        })
    }
    #[cfg(not(windows))]
    0
}

/// `stardust-open --open-cursor` — slim `fs-viz` cannot RegisterWindowMessage.
pub fn open_cursor_cli() -> i32 {
    let out = |v: &Value| {
        if let Some(p) = std::env::var_os("STARDUST_OPEN_CURSOR_OUT") {
            let _ = fs::write(p, serde_json::to_vec(v).unwrap_or_default());
        }
    };
    if find_cursor_exe().is_none() {
        out(&json!({
            "ok": false,
            "status": 400,
            "error": "Cursor.exe not found"
        }));
        return 1;
    }
    match post_open_cursor() {
        Ok(()) => {
            out(&json!({ "ok": true }));
            0
        }
        Err(e) if e == "no StarDustHost" => {
            out(&json!({ "ok": false, "status": 503, "error": e }));
            1
        }
        Err(e) => {
            out(&json!({ "ok": false, "status": 500, "error": e }));
            1
        }
    }
}

/// `PostMessage` Open in Cursor to `StarDustHost`. Does not `HWND_TOP` the cover.
pub fn post_open_cursor() -> Result<(), String> {
    #[cfg(windows)]
    {
        desktop_cover::post_open_cursor_message(wm_open_cursor()).map_err(|e| e.to_string())
    }
    #[cfg(not(windows))]
    Err("no StarDustHost".into())
}

/// View-thread launch: ShellExecute then yield cover z-order ~4s.
pub fn open_cursor_from_view() {
    if open_cursor_gui().is_ok() {
        note_opening_cursor();
    }
}

/// Launch a GUI exe with extra flags then one path argument (no `CREATE_NO_WINDOW` — that hides Electron).
pub fn shell_open_gui(exe: &Path, arg: &Path, extra: &[&str]) -> io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        let mut file: Vec<u16> = exe.as_os_str().encode_wide().collect();
        file.push(0);
        let mut params: Vec<u16> = shell_gui_params(arg, extra).encode_utf16().collect();
        params.push(0);
        let verb: Vec<u16> = "open\0".encode_utf16().collect();
        unsafe {
            let n = windows_sys::Win32::UI::Shell::ShellExecuteW(
                std::ptr::null_mut(),
                verb.as_ptr(),
                file.as_ptr(),
                params.as_ptr(),
                std::ptr::null(),
                windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL,
            ) as isize;
            if n <= 32 {
                return Err(io::Error::other(format!("ShellExecuteW {n}")));
            }
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        Command::new(exe).args(extra).arg(arg).spawn().map(|_| ())
    }
}

/// Open a filesystem folder in Explorer without going through our `open` verb.
pub fn open_in_explorer(path: &Path) -> io::Result<()> {
    #[cfg(windows)]
    {
        windows_explore(path)
    }
    #[cfg(not(windows))]
    {
        let _ = Command::new("xdg-open").arg(path).spawn()?;
        Ok(())
    }
}

#[cfg(windows)]
fn windows_explore(path: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    let mut p: Vec<u16> = path.as_os_str().encode_wide().collect();
    p.push(0);
    let verb: Vec<u16> = "explore\0".encode_utf16().collect();
    unsafe {
        let n = windows_sys::Win32::UI::Shell::ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            p.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL,
        ) as isize;
        if n <= 32 {
            let sys = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
            let mut cmd = Command::new(PathBuf::from(sys).join("explorer.exe"));
            cmd.arg(path);
            let status = spawn_no_window(&mut cmd);
            return status.map(|_| ());
        }
    }
    Ok(())
}

pub fn start_host_headless() -> io::Result<()> {
    if http_ok() {
        return Ok(());
    }
    let fs_viz = sibling_exe("fs-viz");
    let mut cmd = Command::new(&fs_viz);
    cmd.arg(HOST_HEADLESS_ARG);
    spawn_no_window(&mut cmd)?;
    Ok(())
}

/// StarDust UI cover at login / tray. Always spawn `--desktop` (an already-running
/// HTTP server still opens the WebView2 cover). Do not `taskkill` during boot grace.
pub fn start_host_desktop() -> io::Result<()> {
    let fs_viz = sibling_exe("fs-viz");
    let mut cmd = Command::new(&fs_viz);
    cmd.arg(HOST_DESKTOP_ARG);
    spawn_no_window(&mut cmd)?;
    Ok(())
}

/// `true` if StarDust mode should relaunch `--desktop` (cover), not `--headless`.
fn restart_host_uses_desktop_cover(mode: &UiModeFile) -> bool {
    mode.mode == UiMode::Stardust
}

/// Kill and relaunch the host. Returns `true` if a restart was attempted.
/// Rebuild hold: return `false` without `taskkill` — callers must not treat that as a failed restart.
pub fn restart_host() -> bool {
    let mode = load_mode();
    if in_rebuild_hold_at(&mode, unix_now()) {
        append_log("restart-host-skipped-rebuild-hold");
        return false;
    }
    append_log("restart-host");
    #[cfg(windows)]
    {
        let mut kill = Command::new("taskkill");
        kill.args(["/IM", "fs-viz.exe", "/F"]);
        windows_no_window(&mut kill);
        let _ = kill.status();
        std::thread::sleep(Duration::from_millis(400));
    }
    if restart_host_uses_desktop_cover(&mode) {
        let _ = start_host_desktop();
    } else {
        let _ = start_host_headless();
    }
    let _ = wait_http(Duration::from_secs(BOOT_GRACE_SECS));
    true
}

/// StarDust replace-armed: never ShellExecute `explore` / `explorer.exe`.
fn explorer_fallback_blocked(mode: &UiModeFile) -> bool {
    mode.mode == UiMode::Stardust && mode.replace_armed
}

fn path_is_desktop_folder(path: &Path, desktop: &Path) -> bool {
    fn norm(p: &Path) -> String {
        path_string(p)
            .trim_end_matches(['\\', '/'])
            .to_ascii_lowercase()
    }
    let a = norm(path);
    let b = norm(desktop);
    !a.is_empty() && a == b
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NativeFolderPlan {
    /// Windows UI / unarmed: HTTP-down may open Explorer.
    ExplorerFallback,
    /// StarDust desktop path: `--desktop` cover only.
    DesktopCover,
    /// StarDust real folder: `--open PATH` after host HTTP (never Explorer).
    HostOpen,
}

fn native_folder_plan(mode: &UiModeFile, path: &Path, desktop: &Path) -> NativeFolderPlan {
    if !explorer_fallback_blocked(mode) {
        return NativeFolderPlan::ExplorerFallback;
    }
    if path_is_desktop_folder(path, desktop) {
        NativeFolderPlan::DesktopCover
    } else {
        NativeFolderPlan::HostOpen
    }
}

fn spawn_host_open(path: &Path) -> io::Result<()> {
    let fs_viz = sibling_exe("fs-viz");
    let mut cmd = Command::new(&fs_viz);
    cmd.arg("--open").arg(path);
    spawn_no_window(&mut cmd)?;
    Ok(())
}

pub fn open_native_folder(path: &Path) -> io::Result<()> {
    let mode = load_mode();
    let desktop = desktop_dir();
    match native_folder_plan(&mode, path, &desktop) {
        NativeFolderPlan::ExplorerFallback => {
            start_host_headless()?;
            if !wait_http(OPEN_TIMEOUT) {
                note_trampoline_timeout();
                return open_in_explorer(path);
            }
            spawn_host_open(path)
        }
        NativeFolderPlan::DesktopCover => {
            start_host_desktop()?;
            Ok(())
        }
        NativeFolderPlan::HostOpen => {
            if !http_ok() {
                let _ = start_host_desktop();
                let _ = wait_http(OPEN_TIMEOUT);
            }
            if http_ok() {
                spawn_host_open(path)
            } else {
                let _ = start_host_desktop();
                Ok(())
            }
        }
    }
}

/// `stardust-open --apply-mode stardust|windows [--replace] [--hide-icons]`
/// Writes JSON to `STARDUST_UI_MODE_OUT` (slim `fs-viz` cannot call `apply_mode`).
pub fn apply_mode_cli(rest: &[String]) -> i32 {
    let mode_s = rest.first().map(String::as_str).unwrap_or("");
    let Some(mode) = UiMode::parse(mode_s) else {
        write_mode_cli_out(&json!({
            "ok": false,
            "error": "mode must be stardust or windows"
        }));
        return 1;
    };
    let replace = rest.iter().any(|a| a == "--replace");
    let hide = rest.iter().any(|a| a == "--hide-icons");
    match apply_ui_mode_request(mode, replace, hide) {
        Ok((file, warning)) => {
            let mut v = json!({
                "ok": true,
                "mode": file.mode.as_str(),
                "replaceArmed": file.replace_armed,
                "hideDesktopIcons": file.hide_desktop_icons,
            });
            if let Some(w) = warning {
                v["warning"] = json!(w);
            }
            write_mode_cli_out(&v);
            0
        }
        Err(e) => {
            write_mode_cli_out(&json!({ "ok": false, "error": e }));
            1
        }
    }
}

pub fn apply_ui_mode_request(
    mode: UiMode,
    replace: bool,
    hide: bool,
) -> Result<(UiModeFile, Option<String>), String> {
    let replace = replace && mode == UiMode::Stardust;
    match apply_mode(mode, replace, hide) {
        Ok(file) => Ok((file, None)),
        Err(e) if replace => {
            let file = apply_mode(mode, false, false)?;
            Ok((file, Some(e)))
        }
        Err(e) => Err(e),
    }
}

fn write_mode_cli_out(v: &Value) {
    let Some(p) = std::env::var_os("STARDUST_UI_MODE_OUT") else {
        return;
    };
    let _ = fs::write(p, serde_json::to_vec(v).unwrap_or_default());
}

pub fn trampoline_main(args: &[String]) {
    enable_dpi_aware();
    if args.first().map(|s| s.as_str()) == Some("--rebuild-ui") {
        std::process::exit(crate::rebuild::rebuild_ui_cli());
    }
    if args.first().map(|s| s.as_str()) == Some("--open-cursor") {
        std::process::exit(open_cursor_cli());
    }
    if args.first().map(|s| s.as_str()) == Some("--apply-mode") {
        std::process::exit(apply_mode_cli(&args[1..]));
    }
    let _ = ensure_config_dir();
    if std::env::var_os("STARDUST_OPEN").is_some() {
        append_log("trampoline-nested");
        let mode = load_mode();
        if explorer_fallback_blocked(&mode) {
            let _ = start_host_desktop();
            return;
        }
        if let Some(p) = args.iter().find(|a| Path::new(*a).exists()) {
            let _ = open_in_explorer(Path::new(p));
        } else {
            let _ = open_in_explorer(&desktop_dir());
        }
        return;
    }
    std::env::set_var("STARDUST_OPEN", "1");
    if record_open_burst() {
        append_log("open-storm");
        failover_to_windows("open-storm");
        if let Some(p) = args.iter().find(|a| Path::new(*a).exists()) {
            let _ = open_in_explorer(Path::new(p));
        }
        return;
    }
    let mode = load_mode();
    let path = args
        .iter()
        .find(|a| !a.starts_with('-') && Path::new(*a).exists())
        .cloned()
        .or_else(|| args.get(1).cloned());
    let Some(path) = path else {
        if explorer_fallback_blocked(&mode) {
            let _ = open_native_folder(&desktop_dir());
        } else {
            let _ = open_in_explorer(&desktop_dir());
        }
        return;
    };
    let p = PathBuf::from(&path);
    if explorer_fallback_blocked(&mode) {
        let _ = open_native_folder(&p);
        return;
    }
    let _ = open_in_explorer(&p);
}

pub fn probe() -> HealthFile {
    let mode = load_mode();
    let server_ok = http_ok();
    let heartbeat_ok = file_age_secs(&heartbeat_path(), HEARTBEAT_STALE);
    let ui_pulse_ok = file_age_secs(&ui_pulse_path(), UI_PULSE_STALE);
    let backup_ok = backup_path().is_file() || !mode.replace_armed;
    let registry_ok = if mode.replace_armed && mode.mode == UiMode::Stardust {
        registry_matches_trampoline()
    } else {
        !(mode.mode == UiMode::Windows && mode.replace_armed)
    };
    let trampoline_fail = file_age_secs(
        &config_dir().join("trampoline-fail.json"),
        Duration::from_secs(15),
    );
    let icons_mismatch = desktop_icons_should_be_hidden() && desktop_icons_are_visible();
    classify_health(
        &mode,
        server_ok,
        heartbeat_ok,
        ui_pulse_ok,
        backup_ok,
        registry_ok,
        trampoline_fail,
        icons_mismatch,
        unix_now(),
        guard_boot_at(),
    )
}

#[allow(clippy::too_many_arguments)]
fn classify_health(
    mode: &UiModeFile,
    server_ok: bool,
    heartbeat_ok: bool,
    ui_pulse_ok: bool,
    backup_ok: bool,
    registry_ok: bool,
    trampoline_fail: bool,
    icons_mismatch: bool,
    now: u64,
    boot_at: u64,
) -> HealthFile {
    let mut color = "green";
    let mut reason = String::new();
    let startup_grace = in_arm_grace_at(mode, now)
        || in_boot_grace_since(boot_at, now)
        || in_rebuild_hold_at(mode, now);
    if mode.mode == UiMode::Stardust && mode.replace_armed {
        if !backup_ok {
            color = "red";
            reason = "missing-backup".into();
        } else if !registry_ok {
            color = "red";
            reason = "registry-drift".into();
        } else if trampoline_fail {
            color = "red";
            reason = "trampoline-timeout".into();
        } else if startup_grace {
            // Host / WebView2 / first UI pulse may still be coming up after arming or at login.
        } else if !server_ok {
            color = "yellow";
            reason = "http-down".into();
        } else if !heartbeat_ok {
            color = "yellow";
            reason = "heartbeat-stale".into();
        } else if !ui_pulse_ok && !(server_ok && heartbeat_ok) {
            // Edge --app throttles page timers when unfocused; host heartbeat is enough.
            color = "yellow";
            reason = "ui-pulse-stale".into();
        } else if icons_mismatch {
            color = "yellow";
            reason = "icons-mismatch".into();
        }
    } else if mode.mode == UiMode::Stardust && !server_ok && !startup_grace {
        color = "yellow";
        reason = "http-down".into();
    }
    HealthFile {
        color: color.into(),
        reason,
        server_ok,
        heartbeat_ok,
        ui_pulse_ok,
        registry_ok,
        backup_ok,
        ts: now,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct WindowSnapPref {
    win_arranging: bool,
    snap_sizing: bool,
    dock_moving: bool,
}

const SNAP_ON: WindowSnapPref = WindowSnapPref {
    win_arranging: true,
    snap_sizing: true,
    dock_moving: true,
};

fn snap_pref_has_on(p: WindowSnapPref) -> bool {
    p.win_arranging || p.snap_sizing || p.dock_moving
}

fn capture_window_snap_pref(
    saved: bool,
    stored: WindowSnapPref,
    current: WindowSnapPref,
) -> WindowSnapPref {
    if saved {
        stored
    } else if snap_pref_has_on(current) {
        current
    } else {
        // Snap is already off (or GET failed). Do not store that as the user preference.
        SNAP_ON
    }
}

/// Windows UI: saved user flags if any were on; otherwise Windows defaults (all on).
/// Never no-op: a cleared `snap_pref_saved` after a failed restore would leave Snap off.
fn snap_restore_pref(saved: bool, stored: WindowSnapPref) -> WindowSnapPref {
    if saved && snap_pref_has_on(stored) {
        stored
    } else {
        SNAP_ON
    }
}

#[cfg(windows)]
fn spi_get_bool(action: u32) -> bool {
    use windows_sys::Win32::Foundation::BOOL;
    use windows_sys::Win32::UI::WindowsAndMessaging::SystemParametersInfoW;
    let mut v: BOOL = 0;
    let ok = unsafe { SystemParametersInfoW(action, 0, (&mut v as *mut BOOL).cast(), 0) };
    ok != 0 && v != 0
}

#[cfg(windows)]
fn spi_set_bool(action: u32, on: bool) {
    use windows_sys::Win32::Foundation::BOOL;
    use windows_sys::Win32::UI::WindowsAndMessaging::SystemParametersInfoW;
    const SPIF_UPDATEINIFILE: u32 = 0x0001;
    const SPIF_SENDCHANGE: u32 = 0x0002;
    let ui = if on { 1u32 } else { 0 };
    let mut v: BOOL = if on { 1 } else { 0 };
    // Wine/Win32: some SET snap flags read uiParam; MSDN says pvParam BOOL.
    let _ = unsafe {
        SystemParametersInfoW(
            action,
            ui,
            (&mut v as *mut BOOL).cast(),
            SPIF_UPDATEINIFILE | SPIF_SENDCHANGE,
        )
    };
}

#[cfg(windows)]
fn window_snap_pref_now() -> WindowSnapPref {
    const SPI_GETWINARRANGING: u32 = 0x0082;
    const SPI_GETSNAPSIZING: u32 = 0x008E;
    const SPI_GETDOCKMOVING: u32 = 0x0090;
    WindowSnapPref {
        win_arranging: spi_get_bool(SPI_GETWINARRANGING),
        snap_sizing: spi_get_bool(SPI_GETSNAPSIZING),
        dock_moving: spi_get_bool(SPI_GETDOCKMOVING),
    }
}

#[cfg(windows)]
fn set_window_snap_pref(pref: WindowSnapPref) {
    const SPI_SETWINARRANGING: u32 = 0x0083;
    const SPI_SETSNAPSIZING: u32 = 0x008F;
    const SPI_SETDOCKMOVING: u32 = 0x0091;
    spi_set_bool(SPI_SETWINARRANGING, pref.win_arranging);
    spi_set_bool(SPI_SETSNAPSIZING, pref.snap_sizing);
    spi_set_bool(SPI_SETDOCKMOVING, pref.dock_moving);
}

fn snap_pref_from_mode(file: &UiModeFile) -> WindowSnapPref {
    WindowSnapPref {
        win_arranging: file.snap_win_arranging,
        snap_sizing: file.snap_sizing,
        dock_moving: file.snap_dock_moving,
    }
}

fn store_snap_pref(file: &mut UiModeFile, pref: WindowSnapPref) {
    file.snap_pref_saved = true;
    file.snap_win_arranging = pref.win_arranging;
    file.snap_sizing = pref.snap_sizing;
    file.snap_dock_moving = pref.dock_moving;
}

/// StarDust UI: remember the user's Snap flags once, then turn Snap off.
fn disable_window_snap_for_stardust(file: &mut UiModeFile) {
    #[cfg(windows)]
    {
        let stored = snap_pref_from_mode(file);
        let pref = capture_window_snap_pref(file.snap_pref_saved, stored, window_snap_pref_now());
        store_snap_pref(file, pref);
        set_window_snap_pref(WindowSnapPref {
            win_arranging: false,
            snap_sizing: false,
            dock_moving: false,
        });
    }
    #[cfg(not(windows))]
    {
        let _ = file;
    }
}

/// Windows UI: put Snap back to the saved preference (or Windows defaults).
fn restore_window_snap_for_windows(file: &mut UiModeFile) {
    #[cfg(windows)]
    {
        set_window_snap_pref(snap_restore_pref(
            file.snap_pref_saved,
            snap_pref_from_mode(file),
        ));
        file.snap_pref_saved = false;
    }
    #[cfg(not(windows))]
    {
        let _ = file;
    }
}

/// Login / `--desktop`: keep Snap off while mode is already StarDust.
pub fn ensure_stardust_window_snap() {
    SNAP_WINDOWS_APPLIED.store(false, Ordering::SeqCst);
    let mut file = load_mode();
    if file.mode != UiMode::Stardust {
        return;
    }
    disable_window_snap_for_stardust(&mut file);
    let _ = save_mode(&file);
}

/// Windows UI: restore Snap once per stretch (guard tick / float after failsafe).
pub fn ensure_windows_window_snap() {
    let mut file = load_mode();
    if file.mode != UiMode::Windows {
        SNAP_WINDOWS_APPLIED.store(false, Ordering::SeqCst);
        return;
    }
    if SNAP_WINDOWS_APPLIED.swap(true, Ordering::SeqCst) {
        return;
    }
    restore_window_snap_for_windows(&mut file);
    let _ = save_mode(&file);
}

pub fn failover_to_windows(reason: &str) {
    append_log(&format!("failover {reason}"));
    let mut mode = load_mode();
    let _ = restore_hkcu_backup();
    let _ = delete_folder_open_verbs();
    let _ = set_desktop_icons_hidden(false);
    let _ = remove_ui_run();
    mode.mode = UiMode::Windows;
    mode.replace_armed = false;
    mode.hide_desktop_icons = false;
    mode.restart_used = false;
    mode.armed_at = 0;
    mode.rebuild_hold_until = 0;
    mode.last_failover = reason.into();
    restore_window_snap_for_windows(&mut mode);
    SNAP_WINDOWS_APPLIED.store(true, Ordering::SeqCst);
    // Persist Windows before float so WM_WINDOWPOSCHANGED cover restore cannot recove.
    let _ = save_mode(&mode);
    #[cfg(windows)]
    float_stardust_window();
    write_health(&HealthFile {
        color: "red".into(),
        reason: reason.into(),
        server_ok: http_ok(),
        heartbeat_ok: false,
        ui_pulse_ok: false,
        registry_ok: true,
        backup_ok: backup_path().is_file(),
        ts: unix_now(),
    });
}

pub fn apply_mode(mode: UiMode, arm_replace: bool, hide_icons: bool) -> Result<UiModeFile, String> {
    let mut file = load_mode();
    if mode == UiMode::Windows {
        if file.replace_armed || backup_path().is_file() {
            restore_hkcu_backup().map_err(|e| e.to_string())?;
        }
        let _ = delete_folder_open_verbs();
        let _ = set_desktop_icons_hidden(false);
        let _ = remove_ui_run();
        file.mode = UiMode::Windows;
        file.replace_armed = false;
        file.hide_desktop_icons = false;
        file.restart_used = false;
        file.armed_at = 0;
        file.rebuild_hold_until = 0;
        restore_window_snap_for_windows(&mut file);
        SNAP_WINDOWS_APPLIED.store(true, Ordering::SeqCst);
        // Persist Windows before float so WM_WINDOWPOSCHANGED cover restore cannot recove.
        save_mode(&file).map_err(|e| e.to_string())?;
        #[cfg(windows)]
        float_stardust_window();
        return Ok(file);
    }
    file.mode = UiMode::Stardust;
    file.armed_at = unix_now();
    file.restart_used = false;
    if arm_replace {
        backup_hkcu_if_needed().map_err(|e| e.to_string())?;
        write_trampoline_verbs().map_err(|e| e.to_string())?;
        if !registry_matches_trampoline() {
            let _ = restore_hkcu_backup();
            return Err("registry verify failed".into());
        }
        install_guard_run().map_err(|e| e.to_string())?;
        file.replace_armed = true;
    }
    let _ = install_ui_run();
    if hide_icons {
        set_desktop_icons_hidden(true).map_err(|e| e.to_string())?;
        file.hide_desktop_icons = true;
    } else if file.hide_desktop_icons {
        let _ = set_desktop_icons_hidden(false);
        file.hide_desktop_icons = false;
    }
    disable_window_snap_for_stardust(&mut file);
    save_mode(&file).map_err(|e| e.to_string())?;
    #[cfg(windows)]
    place_stardust_window();
    Ok(file)
}

/// Cargo + relaunch hold (600s + ~60s). Windows mode unchanged.
pub fn begin_rebuild_hold() {
    let mut mode = load_mode();
    if mode.mode != UiMode::Stardust {
        return;
    }
    stamp_rebuild_hold(&mut mode, unix_now());
    let _ = save_mode(&mode);
}

/// Refresh arm grace for a planned HTTP gap (rebuild relaunch). Windows mode unchanged.
/// Also stamps rebuild hold so cargo (minutes) is not covered by 20s arm grace alone.
pub fn bump_arm_grace() {
    let mut mode = load_mode();
    if mode.mode != UiMode::Stardust {
        return;
    }
    let now = unix_now();
    mode.armed_at = now;
    mode.restart_used = false;
    stamp_rebuild_hold(&mut mode, now);
    let _ = save_mode(&mode);
}

fn stamp_rebuild_hold(mode: &mut UiModeFile, now: u64) {
    mode.rebuild_hold_until = now.saturating_add(REBUILD_HOLD_SECS);
}

fn in_arm_grace_at(mode: &UiModeFile, now: u64) -> bool {
    if mode.armed_at == 0 {
        return false;
    }
    now.saturating_sub(mode.armed_at) < ARM_GRACE_SECS
}

fn in_rebuild_hold_at(mode: &UiModeFile, now: u64) -> bool {
    mode.rebuild_hold_until != 0 && now < mode.rebuild_hold_until
}

/// Probe used to zero the hold on first `server_ok`, ending the 660s cargo/sleep window.
/// Hold lasts until `rebuild_hold_until` (`in_rebuild_hold_at`) only. Failover still zeros it.
#[cfg(test)]
fn rebuild_hold_until_after_probe(rebuild_hold_until: u64, _server_ok: bool) -> u64 {
    rebuild_hold_until
}

fn note_guard_boot() {
    let now = unix_now().max(1);
    GUARD_BOOT_AT.store(now, Ordering::SeqCst);
}

fn guard_boot_at() -> u64 {
    GUARD_BOOT_AT.load(Ordering::SeqCst)
}

fn in_boot_grace_since(started_at: u64, now: u64) -> bool {
    if started_at == 0 {
        return false;
    }
    now.saturating_sub(started_at) < BOOT_GRACE_SECS
}

/// Yellow http-down / stale heartbeat must not restart or failover during login boot grace,
/// arm grace, or rebuild hold. Red (missing-backup, registry-drift, trampoline-timeout) still may.
fn yellow_host_recovery_blocked(mode: &UiModeFile, boot_at: u64, now: u64) -> bool {
    in_boot_grace_since(boot_at, now) || in_arm_grace_at(mode, now) || in_rebuild_hold_at(mode, now)
}

/// Yellow ui-pulse-stale / heartbeat-stale: do not taskkill or failover (Edge throttles
/// page timers; sleep/lid ages the 5s heartbeat). http-down still recovers. Red still may.
fn yellow_reason_skips_host_restart(reason: &str) -> bool {
    reason == "ui-pulse-stale" || reason == "heartbeat-stale"
}

/// Yellow http-down must not restart or failover while another user app is FG
/// (exclusive fullscreen can hitch the cover). Red / hotkey / tray / open-storm
/// still may. StarDust / shell / taskbar FG still recovers http-down.
fn yellow_http_down_skips_for_other_app(reason: &str, other_app_fg: bool) -> bool {
    reason == "http-down" && other_app_fg
}

/// After a real restart (or a later yellow tick with `restart_used`): failover only if HTTP
/// is still down. ui-pulse-stale / heartbeat-stale with `server_ok` must not flip to Windows.
fn after_restart_should_failover(health: &HealthFile) -> bool {
    !health.server_ok
}

/// Rebuild-hold skip is not a failed restart — do not write `after-restart:*`.
fn after_host_restart_action(restarted: bool, again: &HealthFile) -> bool {
    restarted && after_restart_should_failover(again)
}

pub fn guard_tick() {
    let mut mode = load_mode();
    if mode.mode == UiMode::Windows {
        let _ = remove_ui_run();
        ensure_windows_window_snap();
        // An Explorer label edit (F2) dies if we ShowWindow / SHChangeNotify
        // the shell while the edit control is focused.
        if !label_edit_blocks_shell_refresh(foreground_focus_is_edit()) {
            if registry_matches_trampoline() && hkcu_verbs_need_restore() {
                let _ = restore_hkcu_backup();
                let _ = delete_folder_open_verbs();
            }
            rebind_desktop_label_edit();
            if desktop_icons_need_unhide() {
                let _ = set_desktop_icons_hidden(false);
            }
        }
        let health = probe();
        write_health(&health);
        return;
    }
    ensure_login_autostart();
    if mode.mode == UiMode::Stardust && mode.replace_armed {
        if !label_edit_blocks_shell_refresh(foreground_focus_is_edit()) && desktop_icons_need_hide()
        {
            let _ = set_desktop_icons_hidden(true);
        }
        if !mode.hide_desktop_icons {
            mode.hide_desktop_icons = true;
            let _ = save_mode(&mode);
        }
        #[cfg(windows)]
        keep_cover_if_wrong();
    }
    let health = probe();
    write_health(&health);
    if mode.mode != UiMode::Stardust {
        return;
    }
    if !mode.replace_armed {
        return;
    }
    if health.reason == "registry-drift" {
        let _ = write_trampoline_verbs();
        if registry_matches_trampoline() {
            append_log("registry-rewritten");
            write_health(&probe());
            return;
        }
        failover_to_windows("registry-drift");
        return;
    }
    if health.color == "red" {
        failover_to_windows(&health.reason);
        return;
    }
    if yellow_host_recovery_blocked(&mode, guard_boot_at(), unix_now()) {
        return;
    }
    if health.color == "yellow" {
        if health.reason == "icons-mismatch" {
            let _ = set_desktop_icons_hidden(true);
            return;
        }
        if yellow_reason_skips_host_restart(&health.reason) {
            return;
        }
        #[cfg(windows)]
        if yellow_http_down_skips_for_other_app(
            &health.reason,
            desktop_cover::other_user_app_is_foreground(),
        ) {
            return;
        }
        if !mode.restart_used {
            mode.restart_used = true;
            let _ = save_mode(&mode);
            let restarted = restart_host();
            if !restarted {
                mode.restart_used = false;
                let _ = save_mode(&mode);
                return;
            }
            let again = probe();
            write_health(&again);
            if after_host_restart_action(true, &again) {
                failover_to_windows(&format!("after-restart:{}", again.reason));
            }
        } else if after_restart_should_failover(&health) {
            failover_to_windows(&health.reason);
        }
        return;
    }
    if mode.restart_used && health.color == "green" {
        mode.restart_used = false;
        let _ = save_mode(&mode);
    }
}

pub fn meta_json(repo_default: &str) -> Value {
    let mode = load_mode();
    let health = probe();
    let desktop = desktop_dir();
    let session = session_root(Path::new(repo_default));
    let (volume_free, volume_total) = volume_space(&session);
    json!({
        "uiMode": mode.mode.as_str(),
        "replaceArmed": mode.replace_armed,
        "hideDesktopIcons": mode.hide_desktop_icons,
        "desktopPath": path_string(&desktop),
        "sessionRoot": path_string(&session),
        "volumeFree": volume_free,
        "volumeTotal": volume_total,
        "failsafeHotkey": FAILSAFE_HOTKEY,
        "health": health,
        "lastFailover": mode.last_failover,
    })
}

pub fn path_string(path: &Path) -> String {
    let s = path.to_string_lossy().into_owned();
    if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else if let Some(rest) = s.strip_prefix(r"\\?\") {
        rest.to_string()
    } else {
        s
    }
}

fn trampoline_command() -> Option<String> {
    let p = sibling_exe("stardust-open");
    if p.is_file() {
        Some(format!("\"{}\" \"%1\"", p.display()))
    } else {
        None
    }
}

#[cfg(windows)]
fn registry_matches_trampoline() -> bool {
    let Some(want) = trampoline_command() else {
        return false;
    };
    read_hkcu_cmd(r"Software\Classes\Directory\shell\open\command").as_deref()
        == Some(want.as_str())
        && read_hkcu_cmd(r"Software\Classes\Drive\shell\open\command").as_deref()
            == Some(want.as_str())
}

#[cfg(not(windows))]
fn registry_matches_trampoline() -> bool {
    true
}

fn backup_hkcu_if_needed() -> io::Result<()> {
    if backup_path().is_file() {
        return Ok(());
    }
    ensure_config_dir()?;
    let b = HkcuBackup {
        directory_open: read_hkcu_cmd(r"Software\Classes\Directory\shell\open\command"),
        drive_open: read_hkcu_cmd(r"Software\Classes\Drive\shell\open\command"),
        hide_icons: read_hide_icons(),
    };
    fs::write(backup_path(), serde_json::to_vec_pretty(&b)?)?;
    Ok(())
}

fn hkcu_cmd_already_restored(subkey: &str, val: Option<&str>) -> bool {
    let cur = read_hkcu_cmd(subkey);
    match val {
        Some(s) if !s.to_ascii_lowercase().contains("stardust-open") => cur.as_deref() == Some(s),
        _ => cur.is_none(),
    }
}

fn hkcu_open_matches_backup(b: &HkcuBackup) -> bool {
    hkcu_cmd_already_restored(
        r"Software\Classes\Directory\shell\open\command",
        b.directory_open.as_deref(),
    ) && hkcu_cmd_already_restored(
        r"Software\Classes\Drive\shell\open\command",
        b.drive_open.as_deref(),
    )
}

fn hkcu_verbs_need_restore() -> bool {
    if !registry_matches_trampoline() {
        return false;
    }
    let Ok(text) = fs::read_to_string(backup_path()) else {
        return true;
    };
    let Ok(b) = serde_json::from_str::<HkcuBackup>(&text) else {
        return true;
    };
    !hkcu_open_matches_backup(&b)
}

fn hkcu_open_verbs_present() -> bool {
    read_hkcu_cmd(r"Software\Classes\Directory\shell\open\command").is_some()
        || read_hkcu_cmd(r"Software\Classes\Drive\shell\open\command").is_some()
}

fn restore_hkcu_backup() -> io::Result<()> {
    let Ok(text) = fs::read_to_string(backup_path()) else {
        if hkcu_open_verbs_present() {
            let _ = delete_folder_open_verbs();
            refresh_shell();
        }
        return Ok(());
    };
    let b: HkcuBackup =
        serde_json::from_str(&text).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    let hide_want = b.hide_icons.unwrap_or(0);
    let hide_ok = read_hide_icons().unwrap_or(0) == hide_want;
    let open_ok = hkcu_open_matches_backup(&b);
    if open_ok && hide_ok && !registry_matches_trampoline() {
        return Ok(());
    }
    let mut changed = false;
    if !open_ok {
        restore_one(
            r"Software\Classes\Directory\shell\open\command",
            b.directory_open.as_deref(),
        )?;
        restore_one(
            r"Software\Classes\Drive\shell\open\command",
            b.drive_open.as_deref(),
        )?;
        changed = true;
    }
    if !hide_ok {
        write_hide_icons(hide_want)?;
        changed = true;
    }
    if registry_matches_trampoline() {
        let _ = delete_folder_open_verbs();
        changed = true;
    }
    if changed {
        refresh_shell();
    }
    Ok(())
}

fn restore_one(subkey: &str, val: Option<&str>) -> io::Result<()> {
    match val {
        Some(s) if !s.to_ascii_lowercase().contains("stardust-open") => write_hkcu_cmd(subkey, s),
        _ => {
            let open_key = subkey.strip_suffix(r"\command").unwrap_or(subkey);
            hkcu_delete_tree(open_key)
        }
    }
}

fn delete_folder_open_verbs() -> io::Result<()> {
    hkcu_delete_tree(r"Software\Classes\Directory\shell\open")?;
    hkcu_delete_tree(r"Software\Classes\Drive\shell\open")?;
    Ok(())
}

fn write_trampoline_verbs() -> io::Result<()> {
    let Some(cmd) = trampoline_command() else {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "stardust-open.exe not found next to fs-viz",
        ));
    };
    write_hkcu_cmd(r"Software\Classes\Directory\shell\open\command", &cmd)?;
    write_hkcu_cmd(r"Software\Classes\Drive\shell\open\command", &cmd)?;
    Ok(())
}

fn install_guard_run() -> io::Result<()> {
    #[cfg(windows)]
    {
        let guard = sibling_exe("stardust-guard");
        if !guard.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "stardust-guard.exe missing",
            ));
        }
        write_hkcu_named(
            r"Software\Microsoft\Windows\CurrentVersion\Run",
            "StarDustGuard",
            &format!("\"{}\"", guard.display()),
        )?;
        close_toplevel_guard_window(true);
        if !guard_window_exists() {
            let mut cmd = Command::new(&guard);
            let _ = spawn_no_window(&mut cmd);
        }
    }
    Ok(())
}

pub fn ensure_login_autostart() {
    match read_mode_file() {
        None if ui_run_installed() => {
            let f = UiModeFile {
                mode: UiMode::Stardust,
                ..UiModeFile::default()
            };
            let _ = save_mode(&f);
            let _ = install_ui_run();
        }
        Some(f) if f.mode == UiMode::Stardust => {
            let _ = install_ui_run();
        }
        _ => {
            let _ = remove_ui_run();
        }
    }
}

fn install_ui_run() -> io::Result<()> {
    #[cfg(windows)]
    {
        let host = sibling_exe("fs-viz");
        if !host.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "fs-viz.exe missing",
            ));
        }
        write_hkcu_named(
            r"Software\Microsoft\Windows\CurrentVersion\Run",
            "StarDustUI",
            &format!("\"{}\" --desktop", host.display()),
        )?;
    }
    Ok(())
}

fn remove_ui_run() -> io::Result<()> {
    hkcu_delete_named(
        r"Software\Microsoft\Windows\CurrentVersion\Run",
        "StarDustUI",
    )
}

#[cfg(windows)]
fn write_hkcu_named(subkey: &str, name: &str, value: &str) -> io::Result<()> {
    hkcu_set(subkey, Some(name), value)
}

#[cfg(not(windows))]
fn write_hkcu_named(_subkey: &str, _name: &str, _value: &str) -> io::Result<()> {
    Ok(())
}

fn hkcu_delete_named(subkey: &str, name: &str) -> io::Result<()> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Registry::{
            RegCloseKey, RegDeleteValueW, RegOpenKeyExW, HKEY_CURRENT_USER, KEY_SET_VALUE,
        };
        let mut hkey = std::ptr::null_mut();
        unsafe {
            let e = RegOpenKeyExW(
                HKEY_CURRENT_USER,
                wide(subkey).as_ptr(),
                0,
                KEY_SET_VALUE,
                &mut hkey,
            );
            if e != 0 {
                return Ok(());
            }
            let _ = RegDeleteValueW(hkey, wide(name).as_ptr());
            RegCloseKey(hkey);
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = (subkey, name);
        Ok(())
    }
}

#[cfg(windows)]
fn read_hkcu_cmd(subkey: &str) -> Option<String> {
    hkcu_get(subkey, None)
}

#[cfg(not(windows))]
fn read_hkcu_cmd(_subkey: &str) -> Option<String> {
    None
}

#[cfg(windows)]
fn write_hkcu_cmd(subkey: &str, value: &str) -> io::Result<()> {
    hkcu_set(subkey, None, value)
}

#[cfg(not(windows))]
fn write_hkcu_cmd(_subkey: &str, _value: &str) -> io::Result<()> {
    Ok(())
}

fn read_hide_icons() -> Option<u32> {
    #[cfg(windows)]
    {
        hkcu_get_dword(
            r"Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced",
            "HideIcons",
        )
    }
    #[cfg(not(windows))]
    {
        None
    }
}

fn write_hide_icons(v: u32) -> io::Result<()> {
    #[cfg(windows)]
    {
        hkcu_set_dword(
            r"Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced",
            "HideIcons",
            v,
        )
    }
    #[cfg(not(windows))]
    {
        let _ = v;
        Ok(())
    }
}

fn hide_icons_dword_matches(hidden: bool) -> bool {
    let want = if hidden { 1u32 } else { 0 };
    match read_hide_icons() {
        Some(v) => v == want,
        None => !hidden,
    }
}

/// True while the foreground thread's focus is a Win32 edit control.
/// Explorer F2 rename is that control; shell refresh and listview ShowWindow
/// cancel it in the same instant (the name blinks and the box closes).
fn foreground_focus_is_edit() -> bool {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            GetClassNameW, GetGUIThreadInfo, GUITHREADINFO,
        };
        unsafe {
            let mut info: GUITHREADINFO = std::mem::zeroed();
            info.cbSize = std::mem::size_of::<GUITHREADINFO>() as u32;
            if GetGUIThreadInfo(0, &mut info) == 0 || info.hwndFocus.is_null() {
                return false;
            }
            let mut buf = [0u16; 64];
            let n = GetClassNameW(info.hwndFocus, buf.as_mut_ptr(), buf.len() as i32);
            if n <= 0 {
                return false;
            }
            let name = String::from_utf16_lossy(&buf[..n as usize]);
            name.eq_ignore_ascii_case("Edit")
        }
    }
    #[cfg(not(windows))]
    {
        false
    }
}

fn label_edit_blocks_shell_refresh(edit_focused: bool) -> bool {
    edit_focused
}

/// Icon visibility is the listview only. `SHELLDLL_DefView` must stay visible:
/// `ShowWindow(SW_HIDE)` on it leaves desktop F2 rename opening and closing
/// even after icons are shown again.
fn desktop_list_matches_visible(want_visible: bool) -> bool {
    #[cfg(windows)]
    {
        let list = desktop_listview_hwnd();
        if list.is_null() {
            return true;
        }
        hwnd_is_visible(list) == want_visible
    }
    #[cfg(not(windows))]
    {
        let _ = want_visible;
        true
    }
}

fn desktop_icons_need_unhide() -> bool {
    !hide_icons_dword_matches(false) || !desktop_list_matches_visible(true)
}

fn desktop_icons_need_hide() -> bool {
    !hide_icons_dword_matches(true) || !desktop_list_matches_visible(false)
}

pub fn set_desktop_icons_hidden(hidden: bool) -> io::Result<()> {
    let want_visible = !hidden;
    #[cfg(windows)]
    if want_visible {
        ensure_defview_shown();
    }
    let vis_ok = desktop_list_matches_visible(want_visible);
    if hide_icons_dword_matches(hidden) && vis_ok {
        return Ok(());
    }
    if !vis_ok {
        let _ = set_desktop_listview_visible(want_visible);
    }
    if !hide_icons_dword_matches(hidden) {
        write_hide_icons(if hidden { 1 } else { 0 })?;
        refresh_shell();
    }
    Ok(())
}

fn desktop_icons_should_be_hidden() -> bool {
    let m = load_mode();
    m.mode == UiMode::Stardust && m.replace_armed
}

fn desktop_icons_are_visible() -> bool {
    #[cfg(windows)]
    {
        hwnd_is_visible(desktop_listview_hwnd())
    }
    #[cfg(not(windows))]
    {
        false
    }
}

#[cfg(windows)]
fn hwnd_is_visible(hwnd: windows_sys::Win32::Foundation::HWND) -> bool {
    !hwnd.is_null()
        && unsafe { windows_sys::Win32::UI::WindowsAndMessaging::IsWindowVisible(hwnd) != 0 }
}

/// Shell "Show desktop icons" command (`SHELLDLL_DefView`). Toggles the listview
/// the same way the desktop menu does. Do not `ShowWindow` that DefView.
const DESKTOP_ICON_TOGGLE: usize = 0x7402;

/// 0 = not stuck, 1 = failed to hide, 2 = failed to show. The other direction may still try.
static DESKTOP_VIS_STUCK: AtomicU8 = AtomicU8::new(0);
static DESKTOP_REBIND_TRIES: AtomicU8 = AtomicU8::new(0);

#[cfg(windows)]
fn ensure_defview_shown() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_SHOW};
    let defview = desktop_defview_hwnd();
    if defview.is_null() || hwnd_is_visible(defview) {
        return;
    }
    unsafe {
        let _ = ShowWindow(defview, SW_SHOW);
    }
}

fn set_desktop_listview_visible(visible: bool) -> bool {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            SendMessageW, ShowWindow, SW_HIDE, SW_SHOW, WM_COMMAND,
        };
        let stuck = if visible { 2 } else { 1 };
        if DESKTOP_VIS_STUCK.load(Ordering::SeqCst) == stuck {
            return false;
        }
        if visible {
            ensure_defview_shown();
        }
        let list = desktop_listview_hwnd();
        if list.is_null() || hwnd_is_visible(list) == visible {
            return false;
        }
        let before = hwnd_is_visible(list);
        let defview = desktop_defview_hwnd();
        if !defview.is_null() {
            unsafe {
                let _ = SendMessageW(defview, WM_COMMAND, DESKTOP_ICON_TOGGLE, 0);
            }
        }
        let list = desktop_listview_hwnd();
        if !list.is_null() && hwnd_is_visible(list) == visible {
            DESKTOP_VIS_STUCK.store(0, Ordering::SeqCst);
            return true;
        }
        // Command did not land. Hide/show the listview only — never DefView.
        if !list.is_null() {
            unsafe {
                let _ = ShowWindow(list, if visible { SW_SHOW } else { SW_HIDE });
            }
        }
        let list = desktop_listview_hwnd();
        let after = !list.is_null() && hwnd_is_visible(list);
        if after != visible && after == before {
            DESKTOP_VIS_STUCK.store(stuck, Ordering::SeqCst);
            append_log("desktop-icons-stuck");
            return false;
        }
        DESKTOP_VIS_STUCK.store(0, Ordering::SeqCst);
        after != before
    }
    #[cfg(not(windows))]
    {
        let _ = visible;
        false
    }
}

/// One repair per Explorer session: `ShowWindow` on `SHELLDLL_DefView` leaves
/// F2 rename creating an edit that closes immediately. Two shell toggles put
/// the listview back on the menu path, then a window prop skips later passes.
#[cfg(windows)]
fn rebind_desktop_label_edit() {
    use windows_sys::Win32::Foundation::HANDLE;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetPropW, SendMessageW, SetPropW, WM_COMMAND,
    };
    if foreground_focus_is_edit() {
        return;
    }
    let defview = desktop_defview_hwnd();
    if defview.is_null() {
        return;
    }
    let key = wide("StarDustDesktopRenameOk");
    unsafe {
        if !GetPropW(defview, key.as_ptr()).is_null() {
            return;
        }
    }
    let list = desktop_listview_hwnd();
    if list.is_null() {
        return;
    }
    if DESKTOP_REBIND_TRIES.fetch_add(1, Ordering::SeqCst) >= 3 {
        return;
    }
    ensure_defview_shown();
    let want_visible = hwnd_is_visible(desktop_listview_hwnd());
    unsafe {
        let _ = SendMessageW(defview, WM_COMMAND, DESKTOP_ICON_TOGGLE, 0);
        let _ = SendMessageW(defview, WM_COMMAND, DESKTOP_ICON_TOGGLE, 0);
    }
    let mut now_visible = hwnd_is_visible(desktop_listview_hwnd());
    if now_visible != want_visible {
        unsafe {
            let _ = SendMessageW(defview, WM_COMMAND, DESKTOP_ICON_TOGGLE, 0);
        }
        now_visible = hwnd_is_visible(desktop_listview_hwnd());
    }
    if now_visible != want_visible {
        return;
    }
    unsafe {
        let _ = SetPropW(defview, key.as_ptr(), 1usize as HANDLE);
    }
}

#[cfg(not(windows))]
fn rebind_desktop_label_edit() {}

#[cfg(windows)]
fn desktop_defview_hwnd() -> windows_sys::Win32::Foundation::HWND {
    use windows_sys::Win32::UI::WindowsAndMessaging::{FindWindowExW, FindWindowW};
    unsafe {
        let progman = FindWindowW(wide("Progman").as_ptr(), std::ptr::null());
        let mut defview = FindWindowExW(
            progman,
            std::ptr::null_mut(),
            wide("SHELLDLL_DefView").as_ptr(),
            std::ptr::null(),
        );
        if defview.is_null() {
            let mut worker = std::ptr::null_mut();
            loop {
                worker = FindWindowExW(
                    std::ptr::null_mut(),
                    worker,
                    wide("WorkerW").as_ptr(),
                    std::ptr::null(),
                );
                if worker.is_null() {
                    break;
                }
                defview = FindWindowExW(
                    worker,
                    std::ptr::null_mut(),
                    wide("SHELLDLL_DefView").as_ptr(),
                    std::ptr::null(),
                );
                if !defview.is_null() {
                    break;
                }
            }
        }
        defview
    }
}

#[cfg(windows)]
fn desktop_listview_hwnd() -> windows_sys::Win32::Foundation::HWND {
    use windows_sys::Win32::UI::WindowsAndMessaging::FindWindowExW;
    unsafe {
        FindWindowExW(
            desktop_defview_hwnd(),
            std::ptr::null_mut(),
            wide("SysListView32").as_ptr(),
            std::ptr::null(),
        )
    }
}

#[cfg(windows)]
fn hwnd_message() -> windows_sys::Win32::Foundation::HWND {
    // Sign-extend -3 to a 64-bit HWND. `HWND_MESSAGE` is `-3i32 as _`; a zero-extended
    // 0xFFFFFFFD is not a valid parent and CreateWindowEx then makes a captioned top-level.
    (-3isize) as windows_sys::Win32::Foundation::HWND
}

#[cfg(windows)]
fn guard_window_exists() -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        FindWindowExW, FindWindowW, GetWindowLongPtrW, GWL_STYLE, WS_CAPTION, WS_POPUP,
    };
    unsafe {
        let class = wide("StarDustGuardWnd");
        if !FindWindowExW(
            hwnd_message(),
            std::ptr::null_mut(),
            class.as_ptr(),
            std::ptr::null(),
        )
        .is_null()
        {
            return true;
        }
        let hwnd = FindWindowW(class.as_ptr(), std::ptr::null());
        if hwnd.is_null() {
            return false;
        }
        let style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;
        (style & WS_CAPTION) == 0 && (style & WS_POPUP) != 0
    }
}

#[cfg(not(windows))]
fn guard_window_exists() -> bool {
    false
}

#[cfg(windows)]
fn work_area() -> (i32, i32, i32, i32) {
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::UI::WindowsAndMessaging::{SystemParametersInfoW, SPI_GETWORKAREA};
    let mut r = RECT {
        left: 0,
        top: 0,
        right: 1280,
        bottom: 720,
    };
    unsafe {
        let _ = SystemParametersInfoW(SPI_GETWORKAREA, 0, (&mut r as *mut RECT).cast(), 0);
    }
    (
        r.left,
        r.top,
        r.right.saturating_sub(r.left),
        r.bottom.saturating_sub(r.top),
    )
}

#[cfg(windows)]
#[path = "desktop_cover.rs"]
mod desktop_cover;
#[cfg(windows)]
use desktop_cover::{
    close_legacy_chrome_app, close_legacy_edge_app, close_toplevel_guard_window,
    find_stardust_hwnd, float_stardust_window, keep_cover_if_wrong, keep_frameless,
    place_stardust_window,
};

#[cfg(windows)]
/// Re-apply the borderless work-area cover to every `StarDustHost`.
/// No-op in Windows UI. Focus loss must not call `float_stardust_window`.
pub fn restore_stardust_cover() {
    if load_mode().mode != UiMode::Stardust {
        return;
    }
    keep_frameless();
    raise_user_dock_hwnds();
}

#[cfg(windows)]
static USER_DOCK_HWNDS: Mutex<Vec<isize>> = Mutex::new(Vec::new());

#[cfg(windows)]
pub fn set_user_dock_hwnds(hwnds: Vec<isize>) {
    *USER_DOCK_HWNDS.lock().unwrap_or_else(|e| e.into_inner()) = hwnds;
}

#[cfg(windows)]
/// HWND_TOP (no activate) so a cover restyle cannot bury docked OS windows.
pub fn raise_user_dock_hwnds() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        IsIconic, IsWindowVisible, SetWindowPos, ShowWindow, HWND_TOP, SWP_NOACTIVATE, SWP_NOMOVE,
        SWP_NOSIZE, SWP_SHOWWINDOW, SW_RESTORE,
    };
    let hwnds = USER_DOCK_HWNDS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    for raw in hwnds {
        if raw == 0 {
            continue;
        }
        let hwnd = raw as windows_sys::Win32::Foundation::HWND;
        unsafe {
            if IsIconic(hwnd) != 0 || IsWindowVisible(hwnd) == 0 {
                let _ = ShowWindow(hwnd, SW_RESTORE);
            }
            let _ = SetWindowPos(
                hwnd,
                HWND_TOP,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
        }
    }
}

#[cfg(windows)]
/// Same as `restore_stardust_cover`, but skip a full restyle when the host is
/// already a visible work-area popup. If Progman/WorkerW is above, restack
/// without pinning z-order on neighbor shuffles.
pub fn restore_stardust_cover_if_wrong() {
    if load_mode().mode != UiMode::Stardust {
        return;
    }
    keep_cover_if_wrong();
}

#[cfg(windows)]
pub fn cover_ime_ui_foreground() -> bool {
    desktop_cover::ime_ui_is_foreground()
}

#[cfg(windows)]
pub fn cover_other_user_app_foreground() -> bool {
    desktop_cover::other_user_app_is_foreground()
}

/// `WM_POWERBROADCAST` (winuser.h). Not always in this crate's windows-sys features.
pub const WM_POWERBROADCAST: u32 = 0x0218;
const PBT_APMRESUMECRITICAL: usize = 0x0006;
const PBT_APMRESUMESUSPEND: usize = 0x0007;
const PBT_APMRESUMEAUTOMATIC: usize = 0x0012;

/// True for sleep/lid resume events. Suspend itself is not a restore.
pub fn is_power_resume(wparam: usize) -> bool {
    matches!(
        wparam,
        PBT_APMRESUMECRITICAL | PBT_APMRESUMESUSPEND | PBT_APMRESUMEAUTOMATIC
    )
}

#[cfg(windows)]
/// True when the desktop cover should ignore min/max/restore `WM_SYSCOMMAND`
/// (Win+D must not iconify the wallpaper). Windows UI still DefWindowProc.
pub fn cover_swallow_host_minimize(is_desktop_host: bool, cmd: u32) -> bool {
    desktop_cover::cover_swallow_minimize(is_desktop_host, cmd)
}

#[cfg(windows)]
/// Thread-local `WH_CBT` (`HCBT_MINMAX` only) on the host UI thread.
pub fn install_cover_cbt_hook() {
    desktop_cover::install_cover_cbt_hook();
}

#[cfg(windows)]
/// `NonRudeHWND` on `StarDustHost` and WebView2 children (before first ShowWindow).
pub fn apply_cover_non_rude(hwnd: windows_sys::Win32::Foundation::HWND) {
    desktop_cover::apply_cover_non_rude(hwnd);
}

#[cfg(windows)]
/// `WM_SHOWWINDOW` hide on the desktop cover (Show Desktop / Win+D).
pub fn cover_refuse_host_show_hide(is_desktop_host: bool, showing: bool) -> bool {
    if load_mode().mode != UiMode::Stardust {
        return false;
    }
    desktop_cover::cover_refuse_host_show_hide(is_desktop_host, showing)
}

#[cfg(windows)]
/// Uncloak / unhide / un-minimize the cover in this message (no wait for the next tick).
pub fn cover_stay_visible_from_show_desktop(
    hwnd: windows_sys::Win32::Foundation::HWND,
    force: bool,
) {
    if load_mode().mode != UiMode::Stardust {
        return;
    }
    desktop_cover::cover_stay_visible_now(hwnd, force);
}

#[cfg(windows)]
/// Same skips as `on_host_shown` restore: composing, IME UI FG, or other-app FG.
pub fn cover_skip_restore_if_wrong() -> bool {
    desktop_cover::skip_restore_if_wrong_now()
}

#[cfg(windows)]
/// True when the desktop cover is off the work area — do not reflow WebView2.
pub fn cover_skip_webview_bounds(hwnd: windows_sys::Win32::Foundation::HWND) -> bool {
    desktop_cover::cover_skip_webview_bounds_now(hwnd)
}

#[cfg(windows)]
pub fn uninstall_cover_cbt_hook() {
    desktop_cover::uninstall_cover_cbt_hook();
}

#[cfg(windows)]
/// `WM_WINDOWPOSCHANGING`: lock cover z-order (`SWP_NOZORDER`). Hold the cover
/// at the work area so Snap cannot squash it (do not rewrite `hwndInsertAfter`).
pub fn pin_stardust_cover_changing(
    hwnd: windows_sys::Win32::Foundation::HWND,
    lparam: windows_sys::Win32::Foundation::LPARAM,
) -> bool {
    if load_mode().mode != UiMode::Stardust {
        return false;
    }
    desktop_cover::apply_cover_windowpos_changing(hwnd, lparam)
}

#[cfg(windows)]
/// `WM_GETMINMAXINFO`: min/max track is the work area so Snap cannot shrink the cover.
pub fn pin_stardust_cover_minmax(lparam: windows_sys::Win32::Foundation::LPARAM) {
    if load_mode().mode != UiMode::Stardust {
        return;
    }
    desktop_cover::apply_cover_minmax(lparam);
}

static IME_COMPOSING: AtomicBool = AtomicBool::new(false);

/// True while JS reports IME/RIME composition (including the 700ms quiet after end).
/// Named event so `fs-viz` HTTP and `stardust-view` share the flag.
pub fn ime_composing() -> bool {
    #[cfg(windows)]
    {
        if let Some(h) = ime_event_handle() {
            const WAIT_OBJECT_0: u32 = 0;
            let st = unsafe { windows_sys::Win32::System::Threading::WaitForSingleObject(h, 0) };
            return st == WAIT_OBJECT_0;
        }
    }
    IME_COMPOSING.load(Ordering::SeqCst)
}

pub fn set_ime_composing(composing: bool) {
    IME_COMPOSING.store(composing, Ordering::SeqCst);
    #[cfg(windows)]
    {
        if let Some(h) = ime_event_handle() {
            unsafe {
                if composing {
                    let _ = windows_sys::Win32::System::Threading::SetEvent(h);
                } else {
                    let _ = windows_sys::Win32::System::Threading::ResetEvent(h);
                }
            }
        }
    }
}

/// Skip WebView `put_Bounds`, `MoveFocus`, and `ExecuteScript` while composing.
pub fn ime_blocks_bounds_focus_script(composing: bool) -> bool {
    composing
}

#[cfg(windows)]
fn ime_event_handle() -> Option<windows_sys::Win32::Foundation::HANDLE> {
    static SLOT: OnceLock<isize> = OnceLock::new();
    let raw = *SLOT.get_or_init(|| {
        let name = wide("Local\\StarDustImeComposing");
        let h = unsafe {
            windows_sys::Win32::System::Threading::CreateEventW(
                std::ptr::null(),
                1,
                0,
                name.as_ptr(),
            )
        };
        h as isize
    });
    if raw == 0 {
        None
    } else {
        Some(raw as windows_sys::Win32::Foundation::HANDLE)
    }
}

#[cfg(windows)]
pub fn sweep_host_tooltips() {
    desktop_cover::sweep_host_tooltips(true);
}

#[cfg(windows)]
pub fn sweep_host_tooltips_on_host_destroy() {
    desktop_cover::sweep_host_tooltips(false);
}

#[cfg(windows)]
pub fn close_native_windows() {
    close_legacy_edge_app();
}

fn refresh_shell() {
    #[cfg(windows)]
    unsafe {
        windows_sys::Win32::UI::Shell::SHChangeNotify(
            windows_sys::Win32::UI::Shell::SHCNE_ASSOCCHANGED as i32,
            windows_sys::Win32::UI::Shell::SHCNF_IDLIST,
            std::ptr::null(),
            std::ptr::null(),
        );
    }
}

#[cfg(windows)]
fn hkcu_get(subkey: &str, name: Option<&str>) -> Option<String> {
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY_CURRENT_USER, KEY_READ, REG_SZ,
    };
    let sk = wide(subkey);
    let mut hkey = std::ptr::null_mut();
    unsafe {
        if RegOpenKeyExW(HKEY_CURRENT_USER, sk.as_ptr(), 0, KEY_READ, &mut hkey) != 0 {
            return None;
        }
        let mut ty = 0u32;
        let mut len = 0u32;
        let nptr = name.map(wide);
        let np = nptr
            .as_ref()
            .map(|v| v.as_ptr())
            .unwrap_or(std::ptr::null());
        let q = RegQueryValueExW(
            hkey,
            np,
            std::ptr::null_mut(),
            &mut ty,
            std::ptr::null_mut(),
            &mut len,
        );
        if q != 0 || ty != REG_SZ {
            RegCloseKey(hkey);
            return None;
        }
        let mut buf = vec![0u8; len as usize];
        if RegQueryValueExW(
            hkey,
            np,
            std::ptr::null_mut(),
            &mut ty,
            buf.as_mut_ptr(),
            &mut len,
        ) != 0
        {
            RegCloseKey(hkey);
            return None;
        }
        RegCloseKey(hkey);
        let u16s: Vec<u16> = buf
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .take_while(|c| *c != 0)
            .collect();
        String::from_utf16(&u16s).ok()
    }
}

#[cfg(windows)]
fn hkcu_set(subkey: &str, name: Option<&str>, value: &str) -> io::Result<()> {
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegCreateKeyExW, RegSetValueExW, HKEY_CURRENT_USER, KEY_WRITE,
        REG_OPTION_NON_VOLATILE, REG_SZ,
    };
    let sk = wide(subkey);
    let mut hkey = std::ptr::null_mut();
    let mut disp = 0u32;
    unsafe {
        let e = RegCreateKeyExW(
            HKEY_CURRENT_USER,
            sk.as_ptr(),
            0,
            std::ptr::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE,
            std::ptr::null(),
            &mut hkey,
            &mut disp,
        );
        if e != 0 {
            return Err(io::Error::from_raw_os_error(e as i32));
        }
        let mut data: Vec<u8> = value
            .encode_utf16()
            .chain(std::iter::once(0))
            .flat_map(|u| u.to_le_bytes())
            .collect();
        let nptr = name.map(wide);
        let np = nptr
            .as_ref()
            .map(|v| v.as_ptr())
            .unwrap_or(std::ptr::null());
        let e = RegSetValueExW(hkey, np, 0, REG_SZ, data.as_mut_ptr(), data.len() as u32);
        RegCloseKey(hkey);
        if e != 0 {
            return Err(io::Error::from_raw_os_error(e as i32));
        }
    }
    Ok(())
}

#[cfg(windows)]
fn hkcu_delete_tree(subkey: &str) -> io::Result<()> {
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegDeleteTreeW, RegOpenKeyExW, HKEY_CURRENT_USER, KEY_ALL_ACCESS,
    };
    let Some((parent, name)) = subkey.rsplit_once('\\') else {
        return Ok(());
    };
    let mut hkey = std::ptr::null_mut();
    unsafe {
        let e = RegOpenKeyExW(
            HKEY_CURRENT_USER,
            wide(parent).as_ptr(),
            0,
            KEY_ALL_ACCESS,
            &mut hkey,
        );
        if e != 0 {
            return Ok(());
        }
        let e = RegDeleteTreeW(hkey, wide(name).as_ptr());
        RegCloseKey(hkey);
        if e != 0 && e != 2 {
            return Err(io::Error::from_raw_os_error(e as i32));
        }
    }
    Ok(())
}

#[cfg(not(windows))]
fn hkcu_delete_tree(_subkey: &str) -> io::Result<()> {
    Ok(())
}

#[cfg(windows)]
fn hkcu_get_dword(subkey: &str, name: &str) -> Option<u32> {
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY_CURRENT_USER, KEY_READ, REG_DWORD,
    };
    let sk = wide(subkey);
    let nm = wide(name);
    let mut hkey = std::ptr::null_mut();
    unsafe {
        if RegOpenKeyExW(HKEY_CURRENT_USER, sk.as_ptr(), 0, KEY_READ, &mut hkey) != 0 {
            return None;
        }
        let mut ty = 0u32;
        let mut buf = [0u8; 4];
        let mut len = 4u32;
        let e = RegQueryValueExW(
            hkey,
            nm.as_ptr(),
            std::ptr::null_mut(),
            &mut ty,
            buf.as_mut_ptr(),
            &mut len,
        );
        RegCloseKey(hkey);
        if e != 0 || ty != REG_DWORD {
            return None;
        }
        Some(u32::from_le_bytes(buf))
    }
}

#[cfg(windows)]
fn hkcu_set_dword(subkey: &str, name: &str, v: u32) -> io::Result<()> {
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegCreateKeyExW, RegSetValueExW, HKEY_CURRENT_USER, KEY_WRITE, REG_DWORD,
        REG_OPTION_NON_VOLATILE,
    };
    let sk = wide(subkey);
    let nm = wide(name);
    let mut hkey = std::ptr::null_mut();
    let mut disp = 0u32;
    unsafe {
        let e = RegCreateKeyExW(
            HKEY_CURRENT_USER,
            sk.as_ptr(),
            0,
            std::ptr::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE,
            std::ptr::null(),
            &mut hkey,
            &mut disp,
        );
        if e != 0 {
            return Err(io::Error::from_raw_os_error(e as i32));
        }
        let bytes = v.to_le_bytes();
        let e = RegSetValueExW(hkey, nm.as_ptr(), 0, REG_DWORD, bytes.as_ptr(), 4);
        RegCloseKey(hkey);
        if e != 0 {
            return Err(io::Error::from_raw_os_error(e as i32));
        }
    }
    Ok(())
}

#[cfg(windows)]
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(not(windows))]
pub fn find_edge() -> Option<PathBuf> {
    let candidates = [
        r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
        r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
        r"C:\Program Files (x86)\Microsoft\EdgeWebView\Application\msedgewebview2.exe",
    ];
    for c in candidates {
        let p = PathBuf::from(c);
        if p.is_file() {
            return Some(p);
        }
    }
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        let p = PathBuf::from(local).join(r"Microsoft\Edge\Application\msedge.exe");
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

fn suppress_edge_translate(profile: &Path) {
    let default_dir = profile.join("Default");
    let _ = fs::create_dir_all(&default_dir);
    let prefs_path = default_dir.join("Preferences");
    let mut prefs = if prefs_path.is_file() {
        fs::read_to_string(&prefs_path)
            .ok()
            .and_then(|s| serde_json::from_str::<Value>(&s).ok())
            .unwrap_or_else(|| json!({}))
    } else {
        json!({})
    };
    prefs["translate"]["enabled"] = json!(false);
    prefs["browser"]["translate_infobars_enabled"] = json!(false);
    if let Ok(body) = serde_json::to_vec_pretty(&prefs) {
        let _ = fs::write(prefs_path, body);
    }
}

pub fn open_webview_url(url: &str) -> io::Result<()> {
    let profile = config_dir().join("webview-profile");
    let _ = fs::create_dir_all(&profile);
    suppress_edge_translate(&profile);
    #[cfg(windows)]
    {
        let desktop = is_desktop_cover_url(url);
        if desktop {
            start_desktop_host_if_needed();
            if !http_ok() {
                let _ = wait_http(Duration::from_secs(DESKTOP_HTTP_WAIT_SECS));
            }
            let host_exists = stardust_host_exists();
            match desktop_cover_open_plan(http_ok(), host_exists) {
                DesktopCoverOpenPlan::ReuseCover => {
                    place_stardust_window();
                    return Ok(());
                }
                DesktopCoverOpenPlan::SpawnView {
                    close_legacy_chrome,
                } => {
                    if close_legacy_chrome {
                        close_legacy_chrome_app();
                    }
                }
            }
        } else {
            close_legacy_edge_app();
        }
        let view = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join("stardust-view.exe")))
            .unwrap_or_else(|| PathBuf::from("stardust-view.exe"));
        if !view.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("missing {} — cargo build -p stardust-ui", view.display()),
            ));
        }
        let mut cmd = Command::new(&view);
        cmd.arg(url).arg("--profile").arg(&profile);
        if load_mode().mode == UiMode::Stardust {
            cmd.arg("--desktop");
        }
        let child = spawn_no_window(&mut cmd)?;
        desktop_cover::note_edge_pid(child.id());
        if load_mode().mode == UiMode::Stardust {
            std::thread::spawn(|| {
                for _ in 0..20 {
                    std::thread::sleep(Duration::from_millis(200));
                    place_stardust_window();
                }
            });
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let Some(edge) = find_edge() else {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "Edge/WebView2 not found",
            ));
        };
        Command::new(edge)
            .arg(format!("--app={url}"))
            .arg(format!("--user-data-dir={}", profile.display()))
            .arg("--new-window")
            .spawn()?;
        Ok(())
    }
}

pub fn folder_url(path: &Path) -> String {
    let q = path_string(path);
    let enc: String = q
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            b' ' => "%20".into(),
            _ => format!("%{b:02X}"),
        })
        .collect();
    format!("http://{BIND}/?open={enc}")
}

pub fn desktop_url() -> String {
    format!("http://{BIND}/?desktop=1")
}

/// Native window URL for `--desktop` / `--open PATH`.
/// Cover (`?desktop=1`) only while StarDust UI is on. `--desktop` with Windows UI
/// must not load the work-area cover (leftover HKCU Run must not look like StarDust UI).
pub fn native_window_url(desktop: bool, open: Option<&Path>, mode: UiMode) -> String {
    if mode == UiMode::Stardust && (desktop || open.is_some()) {
        desktop_url()
    } else if let Some(p) = open {
        folder_url(p)
    } else {
        format!("http://{BIND}/")
    }
}

pub fn desktop_cover_wanted(desktop: bool, mode: UiMode) -> bool {
    desktop && mode == UiMode::Stardust
}

pub fn guard_main() {
    enable_dpi_aware();
    let _ = ensure_config_dir();
    note_guard_boot();
    append_log("guard-start");
    std::thread::spawn(boot_stardust_if_armed);
    #[cfg(windows)]
    windows_guard_loop();
    #[cfg(not(windows))]
    loop {
        guard_tick();
        std::thread::sleep(Duration::from_secs(1));
    }
}

fn boot_stardust_if_armed() {
    let mode = load_mode();
    if mode.mode != UiMode::Stardust {
        return;
    }
    let _ = install_ui_run();
    // Give HKCU Run's `fs-viz --desktop` a moment to bind (longer than OPEN_TIMEOUT, still
    // inside boot grace). If nothing is listening, start `--desktop` — not `--headless`.
    if !http_ok() && !wait_http(Duration::from_secs(8)) {
        let _ = start_host_desktop();
    }
    if !wait_http(Duration::from_secs(BOOT_GRACE_SECS)) {
        return;
    }
    #[cfg(windows)]
    {
        for _ in 0..40 {
            if !find_stardust_hwnd().is_null() {
                place_stardust_window();
                return;
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        if find_stardust_hwnd().is_null() {
            let _ = open_webview_url(&desktop_url());
        }
        place_stardust_window();
    }
}

#[cfg(windows)]
const WM_TRAY: u32 = windows_sys::Win32::UI::WindowsAndMessaging::WM_USER + 1;
#[cfg(windows)]
const TRAY_UID: u32 = 1;
#[cfg(windows)]
const HOTKEY_ID: i32 = 0xD501;
#[cfg(windows)]
const TIMER_ID: usize = 1;
#[cfg(windows)]
const MENU_WINDOWS: usize = 1;
#[cfg(windows)]
const MENU_STARDUST: usize = 2;
#[cfg(windows)]
const MENU_DESKTOP: usize = 3;

#[cfg(windows)]
fn tray_tip() -> String {
    let h = probe();
    let mode = load_mode();
    format!(
        "StarDust Guard · {} · {} {}",
        mode.mode.as_str(),
        h.color,
        h.reason
    )
}

#[cfg(windows)]
fn load_icon_from(path: &Path) -> windows_sys::Win32::UI::WindowsAndMessaging::HICON {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        LoadImageW, IMAGE_ICON, LR_DEFAULTSIZE, LR_LOADFROMFILE,
    };
    if !path.is_file() {
        return std::ptr::null_mut();
    }
    let w: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    unsafe {
        LoadImageW(
            std::ptr::null_mut(),
            w.as_ptr(),
            IMAGE_ICON,
            0,
            0,
            LR_LOADFROMFILE | LR_DEFAULTSIZE,
        )
    }
}

#[cfg(windows)]
fn load_icon_from_pe() -> windows_sys::Win32::UI::WindowsAndMessaging::HICON {
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        LoadImageW, IMAGE_ICON, LR_DEFAULTSIZE, LR_SHARED,
    };
    unsafe {
        LoadImageW(
            GetModuleHandleW(std::ptr::null()),
            // MAKEINTRESOURCE(1): integer resource id, not a dangling address.
            #[allow(clippy::manual_dangling_ptr)]
            {
                1 as *const u16
            },
            IMAGE_ICON,
            0,
            0,
            LR_DEFAULTSIZE | LR_SHARED,
        )
    }
}

/// Luna mark: PE resource, then exe-dir `stardust.ico`, then crate assets. Cached (tray ticks).
#[cfg(windows)]
fn load_stardust_icon() -> windows_sys::Win32::UI::WindowsAndMessaging::HICON {
    use std::sync::atomic::{AtomicIsize, Ordering};
    use std::sync::Once;
    use windows_sys::Win32::System::LibraryLoader::GetModuleFileNameW;

    static ONCE: Once = Once::new();
    static ICON: AtomicIsize = AtomicIsize::new(0);
    ONCE.call_once(|| {
        let mut h = load_icon_from_pe();
        if h.is_null() {
            let mut buf = [0u16; 1024];
            let n = unsafe {
                GetModuleFileNameW(std::ptr::null_mut(), buf.as_mut_ptr(), buf.len() as u32)
            };
            if n > 0 && (n as usize) < buf.len() {
                let exe = String::from_utf16_lossy(&buf[..n as usize]);
                if let Some(dir) = Path::new(&exe).parent() {
                    h = load_icon_from(&dir.join("stardust.ico"));
                }
            }
        }
        if h.is_null() {
            h = load_icon_from(
                &PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("assets")
                    .join("stardust.ico"),
            );
        }
        ICON.store(h as isize, Ordering::Release);
    });
    ICON.load(Ordering::Acquire) as _
}

/// Black luna for the light Windows UI caption. Not the PE / taskbar mark.
#[cfg(windows)]
fn load_caption_icon() -> windows_sys::Win32::UI::WindowsAndMessaging::HICON {
    use std::sync::atomic::{AtomicIsize, Ordering};
    use std::sync::Once;
    use windows_sys::Win32::System::LibraryLoader::GetModuleFileNameW;

    static ONCE: Once = Once::new();
    static ICON: AtomicIsize = AtomicIsize::new(0);
    ONCE.call_once(|| {
        let mut h = std::ptr::null_mut();
        let mut buf = [0u16; 1024];
        let n =
            unsafe { GetModuleFileNameW(std::ptr::null_mut(), buf.as_mut_ptr(), buf.len() as u32) };
        if n > 0 && (n as usize) < buf.len() {
            let exe = String::from_utf16_lossy(&buf[..n as usize]);
            if let Some(dir) = Path::new(&exe).parent() {
                h = load_icon_from(&dir.join("stardust-caption.ico"));
            }
        }
        if h.is_null() {
            h = load_icon_from(
                &PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("assets")
                    .join("stardust-caption.ico"),
            );
        }
        ICON.store(h as isize, Ordering::Release);
    });
    ICON.load(Ordering::Acquire) as _
}

/// ICON_BIG stays white (taskbar / PE). Floated Windows UI uses black ICON_SMALL.
#[cfg(windows)]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub fn apply_host_window_icons(hwnd: windows_sys::Win32::Foundation::HWND, floated: bool) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SendMessageW, ICON_BIG, ICON_SMALL, WM_SETICON,
    };
    let big = load_stardust_icon();
    if !big.is_null() {
        unsafe {
            let _ = SendMessageW(hwnd, WM_SETICON, ICON_BIG as usize, big as isize);
        }
    }
    let small = if floated {
        let cap = load_caption_icon();
        if cap.is_null() {
            big
        } else {
            cap
        }
    } else {
        big
    };
    if !small.is_null() {
        unsafe {
            let _ = SendMessageW(hwnd, WM_SETICON, ICON_SMALL as usize, small as isize);
        }
    }
}

#[cfg(windows)]
fn tray_data(
    hwnd: windows_sys::Win32::Foundation::HWND,
) -> windows_sys::Win32::UI::Shell::NOTIFYICONDATAW {
    use windows_sys::Win32::UI::Shell::{NIF_ICON, NIF_MESSAGE, NIF_TIP, NOTIFYICONDATAW};
    use windows_sys::Win32::UI::WindowsAndMessaging::{LoadIconW, IDI_APPLICATION};
    let mut nid: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
    nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
    nid.hWnd = hwnd;
    nid.uID = TRAY_UID;
    nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
    nid.uCallbackMessage = WM_TRAY;
    let custom = load_stardust_icon();
    nid.hIcon = if custom.is_null() {
        unsafe { LoadIconW(std::ptr::null_mut(), IDI_APPLICATION) }
    } else {
        custom
    };
    let tip = tray_tip();
    for (i, u) in tip.encode_utf16().take(127).enumerate() {
        nid.szTip[i] = u;
    }
    nid
}

#[cfg(windows)]
fn tray_add(hwnd: windows_sys::Win32::Foundation::HWND) {
    let nid = tray_data(hwnd);
    unsafe {
        windows_sys::Win32::UI::Shell::Shell_NotifyIconW(
            windows_sys::Win32::UI::Shell::NIM_ADD,
            &nid,
        );
    }
}

#[cfg(windows)]
fn tray_modify(hwnd: windows_sys::Win32::Foundation::HWND) {
    let nid = tray_data(hwnd);
    unsafe {
        windows_sys::Win32::UI::Shell::Shell_NotifyIconW(
            windows_sys::Win32::UI::Shell::NIM_MODIFY,
            &nid,
        );
    }
}

#[cfg(windows)]
fn tray_delete(hwnd: windows_sys::Win32::Foundation::HWND) {
    let nid = tray_data(hwnd);
    unsafe {
        windows_sys::Win32::UI::Shell::Shell_NotifyIconW(
            windows_sys::Win32::UI::Shell::NIM_DELETE,
            &nid,
        );
    }
}

#[cfg(windows)]
fn tray_popup(hwnd: windows_sys::Win32::Foundation::HWND) {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        AppendMenuW, CreatePopupMenu, DestroyMenu, GetCursorPos, SetForegroundWindow,
        TrackPopupMenu, MF_STRING, TPM_RETURNCMD, TPM_RIGHTBUTTON,
    };
    unsafe {
        let menu = CreatePopupMenu();
        if menu.is_null() {
            return;
        }
        let _ = AppendMenuW(menu, MF_STRING, MENU_STARDUST, wide("StarDust UI").as_ptr());
        let _ = AppendMenuW(menu, MF_STRING, MENU_WINDOWS, wide("Windows UI").as_ptr());
        let _ = AppendMenuW(
            menu,
            MF_STRING,
            MENU_DESKTOP,
            wide("Open Desktop in Explorer").as_ptr(),
        );
        let mut pt = POINT { x: 0, y: 0 };
        let _ = GetCursorPos(&mut pt);
        let _ = SetForegroundWindow(hwnd);
        let cmd = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_RIGHTBUTTON,
            pt.x,
            pt.y,
            0,
            hwnd,
            std::ptr::null(),
        ) as usize;
        let _ = DestroyMenu(menu);
        match cmd {
            MENU_WINDOWS => failover_to_windows("tray"),
            MENU_STARDUST => match apply_mode(UiMode::Stardust, true, true) {
                Ok(_) => {
                    let _ = start_host_desktop();
                }
                Err(e) => append_log(&format!("tray-stardust-fail {e}")),
            },
            MENU_DESKTOP => {
                let _ = open_in_explorer(&desktop_dir());
            }
            _ => {}
        }
    }
}

#[cfg(windows)]
fn windows_guard_loop() {
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        RegisterHotKey, UnregisterHotKey, MOD_CONTROL, MOD_SHIFT,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, KillTimer, LoadCursorW,
        PostQuitMessage, RegisterClassW, SetParent, SetTimer, SetWindowPos, ShowWindow,
        TranslateMessage, CS_HREDRAW, CS_VREDRAW, IDC_ARROW, MSG, SWP_HIDEWINDOW, SWP_NOACTIVATE,
        SWP_NOZORDER, SW_HIDE, WM_DESTROY, WM_HOTKEY, WM_LBUTTONUP, WM_RBUTTONUP, WM_TIMER,
        WNDCLASSW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_POPUP,
    };
    unsafe extern "system" fn wndproc(
        hwnd: HWND,
        msg: u32,
        wparam: windows_sys::Win32::Foundation::WPARAM,
        lparam: windows_sys::Win32::Foundation::LPARAM,
    ) -> windows_sys::Win32::Foundation::LRESULT {
        match msg {
            WM_TIMER => {
                guard_tick();
                tray_modify(hwnd);
                0
            }
            WM_HOTKEY => {
                failover_to_windows("hotkey");
                0
            }
            WM_TRAY => {
                let mouse = lparam as u32;
                if mouse == WM_RBUTTONUP {
                    tray_popup(hwnd);
                } else if mouse == WM_LBUTTONUP {
                    let mode = load_mode();
                    if mode.mode == UiMode::Stardust {
                        let _ = open_native_folder(&desktop_dir());
                    } else {
                        let _ = open_in_explorer(&desktop_dir());
                    }
                }
                0
            }
            WM_DESTROY => {
                tray_delete(hwnd);
                let _ = UnregisterHotKey(hwnd, HOTKEY_ID);
                let _ = KillTimer(hwnd, TIMER_ID);
                PostQuitMessage(0);
                0
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
    close_toplevel_guard_window(true);
    let class = wide("StarDustGuardWnd");
    let hinstance =
        unsafe { windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(std::ptr::null()) };
    let wc = WNDCLASSW {
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(wndproc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: hinstance,
        hIcon: std::ptr::null_mut(),
        hCursor: unsafe { LoadCursorW(std::ptr::null_mut(), IDC_ARROW) },
        hbrBackground: std::ptr::null_mut(),
        lpszMenuName: std::ptr::null(),
        lpszClassName: class.as_ptr(),
    };
    unsafe {
        RegisterClassW(&wc);
        let msg_parent = hwnd_message();
        let hwnd = CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            class.as_ptr(),
            std::ptr::null(),
            WS_POPUP,
            -32000,
            -32000,
            0,
            0,
            msg_parent,
            std::ptr::null_mut(),
            hinstance,
            std::ptr::null(),
        );
        if !hwnd.is_null() {
            let _ = SetParent(hwnd, msg_parent);
            let _ = SetWindowPos(
                hwnd,
                std::ptr::null_mut(),
                -32000,
                -32000,
                0,
                0,
                SWP_NOZORDER | SWP_NOACTIVATE | SWP_HIDEWINDOW,
            );
            let _ = ShowWindow(hwnd, SW_HIDE);
        }
        tray_add(hwnd);
        let _ = RegisterHotKey(hwnd, HOTKEY_ID, MOD_CONTROL | MOD_SHIFT, 0x7B);
        let _ = SetTimer(hwnd, TIMER_ID, 1000, None);
        guard_tick();
        let mut msg = std::mem::zeroed::<MSG>();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_mode_names() {
        assert_eq!(UiMode::parse("stardust"), Some(UiMode::Stardust));
        assert_eq!(UiMode::parse("Windows"), Some(UiMode::Windows));
        assert_eq!(UiMode::parse("nope"), None);
        assert_eq!(apply_mode_cli(&["nope".into()]), 1);
    }

    #[test]
    fn label_edit_blocks_desktop_shell_refresh() {
        assert!(label_edit_blocks_shell_refresh(true));
        assert!(!label_edit_blocks_shell_refresh(false));
    }

    #[test]
    fn caption_icon_is_not_the_pe_mark() {
        let assets = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets");
        let pe = assets.join("stardust.ico");
        let cap = assets.join("stardust-caption.ico");
        assert!(pe.is_file());
        assert!(cap.is_file());
        assert_ne!(pe.file_name(), cap.file_name());
    }

    #[test]
    fn power_resume_is_not_suspend() {
        assert!(is_power_resume(0x0006));
        assert!(is_power_resume(0x0007));
        assert!(is_power_resume(0x0012));
        assert!(!is_power_resume(0x0004));
        assert_eq!(WM_POWERBROADCAST, 0x0218);
    }

    #[test]
    fn ime_blocks_bounds_focus_script_follows_composing() {
        assert!(!ime_blocks_bounds_focus_script(false));
        assert!(ime_blocks_bounds_focus_script(true));
    }

    #[test]
    fn gui_params_are_flags_then_quoted_folder() {
        let folder = Path::new(r"C:\Users\Peter\Desktop\StarDust");
        assert_eq!(
            shell_gui_params(folder, &["--classic", "--new-window"]),
            r#"--classic --new-window "C:\Users\Peter\Desktop\StarDust""#
        );
        assert_eq!(
            shell_gui_params(folder, &[]),
            r#""C:\Users\Peter\Desktop\StarDust""#
        );
    }

    #[test]
    fn folder_url_encodes() {
        let u = folder_url(Path::new(r"C:\Users\Peter\Desktop"));
        assert!(u.contains("127.0.0.1:6006"));
        assert!(u.contains("open="));
    }

    #[test]
    fn native_window_url_open_stays_cover_in_stardust_mode() {
        let folder = Path::new(r"C:\Users\Peter\Documents");
        assert_eq!(
            native_window_url(true, None, UiMode::Windows),
            format!("http://{BIND}/"),
            "--desktop while Windows UI must not load ?desktop=1"
        );
        assert!(!desktop_cover_wanted(true, UiMode::Windows));
        assert!(desktop_cover_wanted(true, UiMode::Stardust));
        assert_eq!(
            native_window_url(false, Some(folder), UiMode::Stardust),
            desktop_url(),
            "--open + StarDust mode loads the work-area cover"
        );
        assert_eq!(
            native_window_url(false, Some(folder), UiMode::Windows),
            folder_url(folder),
            "--open while Windows UI stays ?open="
        );
        assert_eq!(
            native_window_url(true, Some(folder), UiMode::Windows),
            folder_url(folder),
            "--desktop + --open while Windows UI stays ?open="
        );
        assert_eq!(
            native_window_url(true, Some(folder), UiMode::Stardust),
            desktop_url(),
            "--desktop while StarDust UI uses the cover"
        );
    }

    #[test]
    fn native_view_launch_taskbar_follows_saved_mode() {
        let cover = desktop_url();
        let home = format!("http://{BIND}/");
        assert_eq!(
            native_view_launch(&cover, true, UiMode::Windows),
            (home.clone(), false),
            "taskbar stardust-view with leftover ?desktop=1 must not cover in Windows UI"
        );
        assert_eq!(
            native_view_launch("", false, UiMode::Windows),
            (home, false)
        );
        assert_eq!(
            native_view_launch("", false, UiMode::Stardust),
            (cover.clone(), true)
        );
        assert_eq!(
            native_view_launch(&cover, true, UiMode::Stardust),
            (cover, true)
        );
    }

    #[cfg(windows)]
    #[test]
    fn message_hwnd_sign_extends() {
        assert_eq!(hwnd_message() as usize, usize::MAX - 2);
    }

    #[test]
    fn desktop_host_arg_is_desktop_not_headless() {
        assert_eq!(HOST_DESKTOP_ARG, "--desktop");
        assert_ne!(HOST_DESKTOP_ARG, HOST_HEADLESS_ARG);
    }

    fn armed_stardust(armed_at: u64, rebuild_hold_until: u64) -> UiModeFile {
        UiModeFile {
            mode: UiMode::Stardust,
            replace_armed: true,
            hide_desktop_icons: true,
            armed_at,
            rebuild_hold_until,
            ..UiModeFile::default()
        }
    }

    fn health_http_down(mode: &UiModeFile, now: u64, boot_at: u64) -> HealthFile {
        classify_health(
            mode, false, false, false, true, true, false, false, now, boot_at,
        )
    }

    #[test]
    fn boot_grace_covers_stale_armed_at_http_down() {
        let mode = armed_stardust(1, 0);
        let now = 1_700_000_000;
        assert!(
            !in_arm_grace_at(&mode, now),
            "stale armed_at must not look like arm grace"
        );
        let boot_at = now;
        assert!(in_boot_grace_since(boot_at, now));
        let health = health_http_down(&mode, now, boot_at);
        assert_eq!(health.color, "green");
        assert!(health.reason.is_empty());
        assert!(!health.server_ok);
        assert!(!yellow_host_recovery_blocked(&mode, 0, now));
        assert!(yellow_host_recovery_blocked(&mode, boot_at, now));
        assert!(!yellow_host_recovery_blocked(
            &mode,
            boot_at,
            now + BOOT_GRACE_SECS
        ));
        let after = health_http_down(&mode, now + BOOT_GRACE_SECS, boot_at);
        assert_eq!(after.color, "yellow");
        assert_eq!(after.reason, "http-down");
    }

    #[test]
    fn boot_grace_still_allows_red_missing_backup() {
        let mode = armed_stardust(1, 0);
        let now = 1_700_000_000;
        let health = classify_health(
            &mode, false, false, false, false, true, false, false, now, now,
        );
        assert_eq!(health.color, "red");
        assert_eq!(health.reason, "missing-backup");
    }

    #[test]
    fn arm_grace_http_down_is_not_yellow() {
        let now = 1_700_000_000;
        let mode = armed_stardust(now, 0);
        assert!(in_arm_grace_at(&mode, now));
        let health = health_http_down(&mode, now, 0);
        assert_eq!(health.color, "green");
        assert!(health.reason.is_empty());
        assert!(yellow_host_recovery_blocked(&mode, 0, now));
        assert!(!yellow_host_recovery_blocked(
            &mode,
            0,
            now + ARM_GRACE_SECS
        ));
        let after = health_http_down(&mode, now + ARM_GRACE_SECS, 0);
        assert_eq!(after.color, "yellow");
        assert_eq!(after.reason, "http-down");
    }

    #[test]
    fn rebuild_hold_http_down_is_not_yellow() {
        let now = 1_700_000_000;
        let mode = armed_stardust(1, now + REBUILD_HOLD_SECS);
        assert!(
            !in_arm_grace_at(&mode, now),
            "stale armed_at: 1 is not arm grace"
        );
        assert!(in_rebuild_hold_at(&mode, now));
        let health = health_http_down(&mode, now, 0);
        assert_eq!(health.color, "green");
        assert!(health.reason.is_empty());
        assert!(yellow_host_recovery_blocked(&mode, 0, now));
        let expired = armed_stardust(1, now);
        assert!(!in_rebuild_hold_at(&expired, now));
        assert!(!yellow_host_recovery_blocked(&expired, 0, now));
        let after = health_http_down(&expired, now, 0);
        assert_eq!(after.color, "yellow");
        assert_eq!(after.reason, "http-down");
        assert_eq!(REBUILD_HOLD_SECS, 660);
    }

    #[test]
    fn server_ok_does_not_clear_unexpired_rebuild_hold() {
        let now = 1_700_000_000;
        let until = now + REBUILD_HOLD_SECS;
        let mode = armed_stardust(1, until);
        assert_eq!(rebuild_hold_until_after_probe(until, true), until);
        assert_eq!(rebuild_hold_until_after_probe(until, false), until);
        assert!(in_rebuild_hold_at(&mode, now));
        assert_eq!(REBUILD_HOLD_SECS, 660);
    }

    fn sample_health(color: &str, reason: &str, server_ok: bool) -> HealthFile {
        HealthFile {
            color: color.into(),
            reason: reason.into(),
            server_ok,
            heartbeat_ok: true,
            ui_pulse_ok: reason != "ui-pulse-stale",
            registry_ok: true,
            backup_ok: true,
            ts: 0,
        }
    }

    #[test]
    fn after_restart_failovers_only_when_http_still_down() {
        let green = sample_health("green", "", true);
        let http_down = sample_health("yellow", "http-down", false);
        let pulse = sample_health("yellow", "ui-pulse-stale", true);
        let heartbeat = sample_health("yellow", "heartbeat-stale", true);
        assert!(!after_restart_should_failover(&green));
        assert!(after_restart_should_failover(&http_down));
        assert!(!after_restart_should_failover(&pulse));
        assert!(!after_restart_should_failover(&heartbeat));
        assert!(after_host_restart_action(true, &http_down));
        assert!(!after_host_restart_action(true, &green));
        assert!(!after_host_restart_action(true, &pulse));
        assert!(
            !after_host_restart_action(false, &http_down),
            "rebuild-hold skip must not failover after-restart"
        );
        assert!(restart_host_uses_desktop_cover(&armed_stardust(1, 0)));
        assert!(!restart_host_uses_desktop_cover(&UiModeFile::default()));
    }

    #[test]
    fn yellow_heartbeat_stale_skips_restart_like_ui_pulse() {
        assert!(yellow_reason_skips_host_restart("ui-pulse-stale"));
        assert!(yellow_reason_skips_host_restart("heartbeat-stale"));
        assert!(!yellow_reason_skips_host_restart("http-down"));
        assert!(!yellow_reason_skips_host_restart("missing-backup"));
        assert!(!yellow_reason_skips_host_restart("open-storm"));
    }

    #[test]
    fn yellow_http_down_skips_restart_when_other_app_fg() {
        assert!(yellow_http_down_skips_for_other_app("http-down", true));
        assert!(!yellow_http_down_skips_for_other_app("http-down", false));
        assert!(!yellow_http_down_skips_for_other_app("open-storm", true));
        assert!(!yellow_http_down_skips_for_other_app(
            "missing-backup",
            true
        ));
        assert!(!yellow_http_down_skips_for_other_app("hotkey", true));
        assert!(!yellow_http_down_skips_for_other_app(
            "ui-pulse-stale",
            true
        ));
        assert!(!yellow_http_down_skips_for_other_app(
            "heartbeat-stale",
            true
        ));
    }

    #[test]
    fn stardust_armed_native_open_never_plans_explorer() {
        let armed = armed_stardust(1, 0);
        let windows = UiModeFile::default();
        let unarmed = UiModeFile {
            mode: UiMode::Stardust,
            replace_armed: false,
            ..UiModeFile::default()
        };
        let desktop = Path::new(r"C:\Users\Peter\Desktop");
        let folder = Path::new(r"C:\Users\Peter\Documents");
        assert!(explorer_fallback_blocked(&armed));
        assert!(!explorer_fallback_blocked(&windows));
        assert!(!explorer_fallback_blocked(&unarmed));
        assert_eq!(
            native_folder_plan(&armed, desktop, desktop),
            NativeFolderPlan::DesktopCover
        );
        assert_eq!(
            native_folder_plan(&armed, folder, desktop),
            NativeFolderPlan::HostOpen
        );
        assert_eq!(
            native_folder_plan(&windows, desktop, desktop),
            NativeFolderPlan::ExplorerFallback
        );
        assert_eq!(
            native_folder_plan(&unarmed, folder, desktop),
            NativeFolderPlan::ExplorerFallback
        );
        assert!(path_is_desktop_folder(desktop, desktop));
        assert!(!path_is_desktop_folder(folder, desktop));
    }

    #[cfg(windows)]
    #[test]
    fn first_frameless_must_not_skip_other_captioned_hosts() {
        assert!(!desktop_cover::hosts_all_cover(&[true, false]));
        assert!(!desktop_cover::hosts_all_cover(&[false, true]));
        assert!(!desktop_cover::hosts_all_cover(&[true, true, false]));
        assert!(!desktop_cover::hosts_all_cover(&[]));
        assert!(!desktop_cover::hosts_all_cover(&[false]));
        assert!(desktop_cover::hosts_all_cover(&[true]));
        assert!(desktop_cover::hosts_all_cover(&[true, true]));
    }

    #[test]
    fn ui_mode_file_serde_defaults_rebuild_hold() {
        let v: UiModeFile = serde_json::from_str(r#"{"mode":"stardust"}"#).unwrap();
        assert_eq!(v.mode, UiMode::Stardust);
        assert_eq!(v.rebuild_hold_until, 0);
        assert!(!v.replace_armed);
        assert!(!v.snap_pref_saved);
    }

    #[test]
    fn snap_pref_is_captured_once() {
        let stored = WindowSnapPref {
            win_arranging: true,
            snap_sizing: true,
            dock_moving: true,
        };
        let already_off = WindowSnapPref {
            win_arranging: false,
            snap_sizing: false,
            dock_moving: false,
        };
        assert_eq!(capture_window_snap_pref(false, stored, stored), stored);
        assert_eq!(
            capture_window_snap_pref(true, stored, already_off),
            stored,
            "second disable must not treat already-off Snap as the user preference"
        );
        assert_eq!(
            capture_window_snap_pref(false, already_off, already_off),
            SNAP_ON,
            "first capture must not store already-off as the user preference"
        );
        assert_eq!(
            snap_restore_pref(false, already_off),
            SNAP_ON,
            "Windows UI with no saved flags must turn Snap on"
        );
        assert_eq!(
            snap_restore_pref(true, already_off),
            SNAP_ON,
            "saved all-off is a failed capture, not the user preference"
        );
        assert_eq!(snap_restore_pref(true, stored), stored);
    }

    #[test]
    fn desktop_cover_reuse_does_not_close_when_http_up() {
        assert_eq!(
            desktop_cover_open_plan(true, true),
            DesktopCoverOpenPlan::ReuseCover
        );
        assert_eq!(
            desktop_cover_open_plan(false, true),
            DesktopCoverOpenPlan::ReuseCover
        );
        assert_eq!(
            desktop_cover_open_plan(true, false),
            DesktopCoverOpenPlan::SpawnView {
                close_legacy_chrome: true
            }
        );
        assert_eq!(
            desktop_cover_open_plan(false, false),
            DesktopCoverOpenPlan::SpawnView {
                close_legacy_chrome: true
            }
        );
        assert!(is_desktop_cover_url("http://127.0.0.1:6006/?desktop=1"));
        assert!(!is_desktop_cover_url("http://127.0.0.1:6006/?open=C"));
    }

    #[test]
    fn http_ready_starts_host_when_stardust_and_down() {
        assert_eq!(http_ready_plan(true, true), HttpReadyPlan::NavigateNow);
        assert_eq!(http_ready_plan(true, false), HttpReadyPlan::NavigateNow);
        assert_eq!(
            http_ready_plan(false, true),
            HttpReadyPlan::StartHostThenWait
        );
        assert_eq!(
            http_ready_plan(false, false),
            HttpReadyPlan::StartHostThenWait
        );
        assert_eq!(DESKTOP_HTTP_WAIT_SECS, 5);
        assert_eq!(BOOT_GRACE_SECS, 60);
    }
}
