//! Clip Edge `--app=` caption/border. Chromium paints that chrome itself; WS_CAPTION
//! strip is not enough. Not a language-wave module.

use std::cell::Cell;
use std::sync::atomic::{AtomicI32, AtomicU32, Ordering};

use super::{append_log, wide, work_area};

type Hwnd = windows_sys::Win32::Foundation::HWND;

static CLIP_L: AtomicI32 = AtomicI32::new(0);
static CLIP_T: AtomicI32 = AtomicI32::new(0);
static CLIP_R: AtomicI32 = AtomicI32::new(0);
static CLIP_B: AtomicI32 = AtomicI32::new(0);
static EDGE_PID: AtomicU32 = AtomicU32::new(0);
static LAST_KIND: AtomicU32 = AtomicU32::new(0);

pub(super) fn note_edge_pid(pid: u32) {
    if pid != 0 {
        EDGE_PID.store(pid, Ordering::Relaxed);
    }
}

fn window_class(hwnd: Hwnd) -> String {
    use windows_sys::Win32::UI::WindowsAndMessaging::GetClassNameW;
    let mut buf = [0u16; 256];
    let n = unsafe { GetClassNameW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
    if n <= 0 {
        return String::new();
    }
    String::from_utf16_lossy(&buf[..n as usize])
}

fn window_title(hwnd: Hwnd) -> String {
    use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowTextW;
    let mut buf = [0u16; 256];
    let n = unsafe { GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
    if n <= 0 {
        return String::new();
    }
    String::from_utf16_lossy(&buf[..n as usize])
}

fn hwnd_pid(hwnd: Hwnd) -> u32 {
    use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;
    let mut pid = 0u32;
    unsafe {
        let _ = GetWindowThreadProcessId(hwnd, &mut pid);
    }
    pid
}

fn window_dpi(hwnd: Hwnd) -> i32 {
    use windows_sys::Win32::Graphics::Gdi::{GetDC, GetDeviceCaps, ReleaseDC, LOGPIXELSY};
    unsafe {
        let hdc = GetDC(hwnd);
        let dpi = GetDeviceCaps(hdc, LOGPIXELSY as i32);
        ReleaseDC(hwnd, hdc);
        dpi.max(96)
    }
}

fn find_child_class(parent: Hwnd, want: &str) -> Hwnd {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetWindow, GW_CHILD, GW_HWNDNEXT};
    unsafe {
        let mut child = GetWindow(parent, GW_CHILD);
        while !child.is_null() {
            if window_class(child).eq_ignore_ascii_case(want) {
                return child;
            }
            let nested = find_child_class(child, want);
            if !nested.is_null() {
                return nested;
            }
            child = GetWindow(child, GW_HWNDNEXT);
        }
        std::ptr::null_mut()
    }
}

fn pad_insets(hwnd: Hwnd, l: i32, t: i32, r: i32, b: i32) -> (i32, i32, i32, i32) {
    let extra = (8 * window_dpi(hwnd) + 95) / 96;
    // Only a few px of DWM caption remain after the measured inset; 12dip ate the search bar.
    let extra_top = (4 * window_dpi(hwnd) + 95) / 96;
    (
        l.max(extra),
        t + extra_top.max(4),
        r.max(extra),
        b.max(extra),
    )
}

fn chrome_insets(hwnd: Hwnd) -> (i32, i32, i32, i32) {
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowRect;
    let stored = (
        CLIP_L.load(Ordering::Relaxed),
        CLIP_T.load(Ordering::Relaxed),
        CLIP_R.load(Ordering::Relaxed),
        CLIP_B.load(Ordering::Relaxed),
    );
    if stored.1 >= 8 {
        return stored;
    }
    let mut wr = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    unsafe {
        let _ = GetWindowRect(hwnd, &mut wr);
    }
    let render = find_child_class(hwnd, "Chrome_RenderWidgetHostHWND");
    if !render.is_null() {
        let mut rr = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        unsafe {
            let _ = GetWindowRect(render, &mut rr);
        }
        let rw = rr.right - rr.left;
        let rh = rr.bottom - rr.top;
        if rw > 400 && rh > 400 {
            let l = (rr.left - wr.left).clamp(0, 16);
            let t = (rr.top - wr.top).clamp(0, 80);
            let r = (wr.right - rr.right).clamp(0, 16);
            let b = (wr.bottom - rr.bottom).clamp(0, 16);
            if t >= 16 {
                let (l, t, r, b) = pad_insets(hwnd, l, t, r, b);
                CLIP_L.store(l, Ordering::Relaxed);
                CLIP_T.store(t, Ordering::Relaxed);
                CLIP_R.store(r, Ordering::Relaxed);
                CLIP_B.store(b, Ordering::Relaxed);
                return (l, t, r, b);
            }
        }
    }
    let dip = (32 * window_dpi(hwnd) + 95) / 96;
    let (l, t, r, b) = pad_insets(hwnd, 0, dip, 0, 0);
    CLIP_L.store(l, Ordering::Relaxed);
    CLIP_T.store(t, Ordering::Relaxed);
    CLIP_R.store(r, Ordering::Relaxed);
    CLIP_B.store(b, Ordering::Relaxed);
    (l, t, r, b)
}

fn reset_clip_insets() {
    CLIP_L.store(0, Ordering::Relaxed);
    CLIP_T.store(0, Ordering::Relaxed);
    CLIP_R.store(0, Ordering::Relaxed);
    CLIP_B.store(0, Ordering::Relaxed);
    LAST_KIND.store(0, Ordering::Relaxed);
}

fn note_cover(kind: u32, msg: &str) {
    if LAST_KIND.swap(kind, Ordering::Relaxed) != kind {
        append_log(msg);
    }
}

unsafe extern "system" fn enum_edge_app(
    hwnd: Hwnd,
    lparam: windows_sys::Win32::Foundation::LPARAM,
) -> windows_sys::Win32::Foundation::BOOL {
    use windows_sys::Win32::UI::WindowsAndMessaging::IsWindowVisible;
    let out = lparam as *mut Hwnd;
    let class = window_class(hwnd);
    if class == "StarDustHost" {
        if unsafe { IsWindowVisible(hwnd) } == 0 {
            return 1;
        }
        unsafe {
            *out = hwnd;
        }
        return 0;
    }
    if class == "StarDustCover" {
        return 1;
    }
    if !class.starts_with("Chrome_WidgetWin") {
        return 1;
    }
    if unsafe { IsWindowVisible(hwnd) } == 0 {
        return 1;
    }
    let title = window_title(hwnd);
    let pid = hwnd_pid(hwnd);
    let want = EDGE_PID.load(Ordering::Relaxed);
    let pid_ok = want != 0 && pid == want;
    if title == "StarDust" || pid_ok {
        unsafe {
            *out = hwnd;
        }
        return 0;
    }
    1
}

fn close_legacy_windows(chrome_only: bool) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{EnumWindows, PostMessageW, WM_CLOSE};
    unsafe extern "system" fn each(
        hwnd: Hwnd,
        lparam: windows_sys::Win32::Foundation::LPARAM,
    ) -> windows_sys::Win32::Foundation::BOOL {
        let chrome_only = lparam != 0;
        let class = window_class(hwnd);
        let chrome = class.starts_with("Chrome_WidgetWin") && window_title(hwnd) == "StarDust";
        let host = class == "StarDustHost";
        if chrome || (!chrome_only && host) {
            unsafe {
                let _ = PostMessageW(hwnd, WM_CLOSE, 0, 0);
            }
        }
        1
    }
    unsafe {
        let _ = EnumWindows(Some(each), if chrome_only { 1 } else { 0 });
    }
}

/// WM_CLOSE leftover Edge `--app=` chrome only — never a live `StarDustHost`.
pub(super) fn close_legacy_chrome_app() {
    close_legacy_windows(true);
}

pub(super) fn close_legacy_edge_app() {
    close_legacy_windows(false);
}

pub(super) fn find_stardust_hwnd() -> Hwnd {
    use windows_sys::Win32::UI::WindowsAndMessaging::EnumWindows;
    let mut found: Hwnd = std::ptr::null_mut();
    unsafe {
        let _ = EnumWindows(Some(enum_edge_app), &mut found as *mut Hwnd as isize);
    }
    found
}

unsafe extern "system" fn enum_stardust_hosts(
    hwnd: Hwnd,
    lparam: windows_sys::Win32::Foundation::LPARAM,
) -> windows_sys::Win32::Foundation::BOOL {
    if window_class(hwnd) != "StarDustHost" {
        return 1;
    }
    let out = lparam as *mut Vec<Hwnd>;
    unsafe {
        (*out).push(hwnd);
    }
    1
}

unsafe extern "system" fn enum_visible_hosts(
    hwnd: Hwnd,
    lparam: windows_sys::Win32::Foundation::LPARAM,
) -> windows_sys::Win32::Foundation::BOOL {
    use windows_sys::Win32::UI::WindowsAndMessaging::IsWindowVisible;
    if window_class(hwnd) != "StarDustHost" {
        return 1;
    }
    if unsafe { IsWindowVisible(hwnd) } == 0 {
        return 1;
    }
    let out = lparam as *mut Vec<Hwnd>;
    unsafe {
        (*out).push(hwnd);
    }
    1
}

fn collect_stardust_hosts(
    callback: unsafe extern "system" fn(
        Hwnd,
        windows_sys::Win32::Foundation::LPARAM,
    ) -> windows_sys::Win32::Foundation::BOOL,
) -> Vec<Hwnd> {
    use windows_sys::Win32::UI::WindowsAndMessaging::EnumWindows;
    let mut found: Vec<Hwnd> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(callback), &mut found as *mut Vec<Hwnd> as isize);
    }
    found
}

/// Every `StarDustHost`, including cloaked/hidden/iconic after exclusive fullscreen.
fn stardust_hosts() -> Vec<Hwnd> {
    collect_stardust_hosts(enum_stardust_hosts)
}

fn visible_stardust_hosts() -> Vec<Hwnd> {
    collect_stardust_hosts(enum_visible_hosts)
}

pub(super) fn native_host_exists() -> bool {
    !stardust_hosts().is_empty()
}

/// True only when every visible host is already a borderless work-area cover.
/// A single frameless hwnd must not hide a captioned sibling.
pub(super) fn hosts_all_cover(frameless: &[bool]) -> bool {
    !frameless.is_empty() && frameless.iter().copied().all(|ok| ok)
}

fn release_cover_hosts() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        DestroyWindow, FindWindowW, GetWindow, SetParent, GW_CHILD, GW_HWNDNEXT,
    };
    loop {
        let cover = unsafe { FindWindowW(wide("StarDustCover").as_ptr(), std::ptr::null()) };
        if cover.is_null() {
            break;
        }
        unsafe {
            let mut child = GetWindow(cover, GW_CHILD);
            while !child.is_null() {
                let next = GetWindow(child, GW_HWNDNEXT);
                let _ = SetParent(child, std::ptr::null_mut());
                child = next;
            }
            let _ = DestroyWindow(cover);
        }
    }
}

fn dwm_set_u32(hwnd: Hwnd, attr: u32, value: u32) {
    use windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute;
    unsafe {
        let _ = DwmSetWindowAttribute(hwnd, attr, (&value as *const u32).cast(), 4);
    }
}

fn dwm_hide_chrome(hwnd: Hwnd) {
    const DWMWA_WINDOW_CORNER_PREFERENCE: u32 = 33;
    const DWMWA_BORDER_COLOR: u32 = 34;
    const DWMWA_CAPTION_COLOR: u32 = 35;
    const DWMWCP_DONOTROUND: u32 = 1;
    dwm_set_u32(hwnd, DWMWA_BORDER_COLOR, 0x000D0705);
    dwm_set_u32(hwnd, DWMWA_CAPTION_COLOR, 0x000D0705);
    dwm_set_u32(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND);
}

fn dwm_restore_chrome(hwnd: Hwnd) {
    const DWMWA_NCRENDERING_POLICY: u32 = 2;
    const DWMNCRP_USEWINDOWSTYLE: u32 = 0;
    const DWMWA_WINDOW_CORNER_PREFERENCE: u32 = 33;
    const DWMWA_BORDER_COLOR: u32 = 34;
    const DWMWA_CAPTION_COLOR: u32 = 35;
    const DWMWA_COLOR_DEFAULT: u32 = 0xFFFFFFFF;
    const DWMWCP_DEFAULT: u32 = 0;
    dwm_set_u32(hwnd, DWMWA_NCRENDERING_POLICY, DWMNCRP_USEWINDOWSTYLE);
    dwm_set_u32(hwnd, DWMWA_BORDER_COLOR, DWMWA_COLOR_DEFAULT);
    dwm_set_u32(hwnd, DWMWA_CAPTION_COLOR, DWMWA_COLOR_DEFAULT);
    dwm_set_u32(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DEFAULT);
}

fn dwm_disable_nc(hwnd: Hwnd) {
    const DWMWA_NCRENDERING_POLICY: u32 = 2;
    const DWMNCRP_DISABLED: u32 = 1;
    dwm_set_u32(hwnd, DWMWA_NCRENDERING_POLICY, DWMNCRP_DISABLED);
}

fn set_desktop_host_flag(hwnd: Hwnd, on: bool) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{SetWindowLongPtrW, GWLP_USERDATA};
    unsafe {
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, if on { 1 } else { 0 });
    }
}

fn set_ex_appwindow(hwnd: Hwnd, on: bool) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetWindowLongPtrW, GWL_EXSTYLE, WS_EX_APPWINDOW, WS_EX_TOPMOST,
    };
    unsafe {
        let mut ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        if on {
            ex |= WS_EX_APPWINDOW as isize;
        } else {
            ex &= !(WS_EX_APPWINDOW as isize);
            ex &= !(WS_EX_TOPMOST as isize);
        }
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex);
    }
}

fn process_image_stem(pid: u32) -> String {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    if pid == 0 {
        return String::new();
    }
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return String::new();
        }
        let mut buf = [0u16; 512];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, buf.as_mut_ptr(), &mut len);
        let _ = CloseHandle(h);
        if ok == 0 || len == 0 {
            return String::new();
        }
        let path = String::from_utf16_lossy(&buf[..len as usize]);
        std::path::Path::new(&path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase()
    }
}

fn is_stardust_pid(pid: u32) -> bool {
    matches!(
        process_image_stem(pid).as_str(),
        "stardust-view" | "stardust-guard" | "fs-viz"
    )
}

fn is_explorer_pid(pid: u32) -> bool {
    process_image_stem(pid) == "explorer"
}

fn process_is_dead(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError};
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    const STILL_ACTIVE: u32 = 259;
    const ERROR_ACCESS_DENIED: u32 = 5;
    if pid == 0 {
        return true;
    }
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return GetLastError() != ERROR_ACCESS_DENIED;
        }
        let mut code = 0u32;
        let ok = GetExitCodeProcess(h, &mut code);
        let _ = CloseHandle(h);
        ok != 0 && code != STILL_ACTIVE
    }
}

fn owner_is_live_floated_host(owner: Hwnd) -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, IsWindow, GWL_STYLE, GWLP_USERDATA, WS_CAPTION,
    };
    if owner.is_null() || unsafe { IsWindow(owner) } == 0 {
        return false;
    }
    if window_class(owner) != "StarDustHost" {
        return false;
    }
    let desktop = unsafe { GetWindowLongPtrW(owner, GWLP_USERDATA) } != 0;
    if desktop {
        return false;
    }
    let style = unsafe { GetWindowLongPtrW(owner, GWL_STYLE) } as u32;
    (style & WS_CAPTION) != 0
}

fn tooltip_should_sweep(hwnd: Hwnd, keep_floated_host: bool) -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetWindow, IsWindow, IsWindowVisible, GW_OWNER,
    };
    let pid = hwnd_pid(hwnd);
    if is_explorer_pid(pid) {
        return false;
    }
    let owner = unsafe { GetWindow(hwnd, GW_OWNER) };
    if !owner.is_null() && is_explorer_pid(hwnd_pid(owner)) {
        return false;
    }
    if keep_floated_host && owner_is_live_floated_host(owner) {
        return false;
    }
    if is_stardust_pid(pid) {
        return true;
    }
    if !owner.is_null() && unsafe { IsWindow(owner) } != 0 {
        let class = window_class(owner);
        if class == "StarDustHost" || class == "StarDustGuardWnd" || is_stardust_pid(hwnd_pid(owner))
        {
            return true;
        }
    }
    if unsafe { IsWindowVisible(hwnd) } == 0 {
        return false;
    }
    owner.is_null() || unsafe { IsWindow(owner) } == 0 || process_is_dead(hwnd_pid(owner))
}

fn close_one_toplevel_guard(hwnd: Hwnd, wait: bool) {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        GetCurrentProcessId, OpenProcess, TerminateProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE,
        PROCESS_TERMINATE,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{DestroyWindow, IsWindow, PostMessageW, WM_CLOSE};
    let pid = hwnd_pid(hwnd);
    unsafe {
        let _ = PostMessageW(hwnd, WM_CLOSE, 0, 0);
    }
    if wait && pid != 0 {
        unsafe {
            let h = OpenProcess(PROCESS_SYNCHRONIZE, 0, pid);
            if !h.is_null() {
                let _ = WaitForSingleObject(h, 1000);
                let _ = CloseHandle(h);
            }
        }
    }
    unsafe {
        if IsWindow(hwnd) != 0 {
            let _ = DestroyWindow(hwnd);
        }
    }
    if wait {
        unsafe {
            if IsWindow(hwnd) != 0
                && pid != 0
                && pid != GetCurrentProcessId()
                && process_image_stem(pid) == "stardust-guard"
            {
                let h = OpenProcess(PROCESS_TERMINATE, 0, pid);
                if !h.is_null() {
                    let _ = TerminateProcess(h, 1);
                    let _ = CloseHandle(h);
                }
            }
        }
    }
}

fn is_legacy_caption_guard(hwnd: Hwnd) -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetWindowLongPtrW, GWL_STYLE, WS_CAPTION, WS_POPUP};
    let style = unsafe { GetWindowLongPtrW(hwnd, GWL_STYLE) } as u32;
    (style & WS_CAPTION) != 0 || (style & WS_POPUP) == 0
}

pub(super) fn close_toplevel_guard_window(wait: bool) {
    use windows_sys::Win32::UI::WindowsAndMessaging::FindWindowW;
    for _ in 0..8 {
        let hwnd = unsafe { FindWindowW(wide("StarDustGuardWnd").as_ptr(), std::ptr::null()) };
        if hwnd.is_null() {
            return;
        }
        if !is_legacy_caption_guard(hwnd) {
            return;
        }
        close_one_toplevel_guard(hwnd, wait);
        let still = unsafe { FindWindowW(wide("StarDustGuardWnd").as_ptr(), std::ptr::null()) };
        if still == hwnd {
            break;
        }
    }
}

pub(super) fn sweep_host_tooltips(keep_floated_host: bool) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        DestroyWindow, FindWindowExW, PostMessageW, WM_CLOSE,
    };
    let class = wide("tooltips_class32");
    let mut found: Vec<Hwnd> = Vec::new();
    let mut prev: Hwnd = std::ptr::null_mut();
    for _ in 0..64 {
        let hwnd = unsafe {
            FindWindowExW(
                std::ptr::null_mut(),
                prev,
                class.as_ptr(),
                std::ptr::null(),
            )
        };
        if hwnd.is_null() {
            break;
        }
        if tooltip_should_sweep(hwnd, keep_floated_host) {
            found.push(hwnd);
        }
        prev = hwnd;
    }
    for hwnd in found {
        unsafe {
            let _ = PostMessageW(hwnd, WM_CLOSE, 0, 0);
            let _ = DestroyWindow(hwnd);
        }
    }
}

fn window_rect(hwnd: Hwnd) -> (i32, i32, i32, i32) {
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowRect;
    let mut r = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    unsafe {
        let _ = GetWindowRect(hwnd, &mut r);
    }
    (
        r.left,
        r.top,
        r.right.saturating_sub(r.left),
        r.bottom.saturating_sub(r.top),
    )
}

fn clip_target(hwnd: Hwnd) -> (i32, i32, i32, i32, i32, i32, i32, i32) {
    let (x, y, w, h) = work_area();
    let (fl, cap, fr, fb) = chrome_insets(hwnd);
    (x - fl, y - cap, w + fl + fr, h + cap + fb, fl, cap, w, h)
}

fn clip_already_ok(hwnd: Hwnd) -> bool {
    use windows_sys::Win32::Graphics::Gdi::{CreateRectRgn, DeleteObject, GetWindowRgn};
    let (tx, ty, tw, th, _, _, _, _) = clip_target(hwnd);
    let (x, y, w, h) = window_rect(hwnd);
    if (x - tx).abs() > 1 || (y - ty).abs() > 1 || (w - tw).abs() > 1 || (h - th).abs() > 1 {
        return false;
    }
    unsafe {
        let rgn = CreateRectRgn(0, 0, 0, 0);
        let ty = GetWindowRgn(hwnd, rgn);
        let _ = DeleteObject(rgn);
        ty > 1
    }
}

/// OS chrome that must not remain on a StarDust work-area cover.
/// `WS_POPUP` can still carry caption/thickframe bits after exclusive-fullscreen restore.
fn cover_chrome_bits() -> u32 {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        WS_CAPTION, WS_MAXIMIZE, WS_MAXIMIZEBOX, WS_MINIMIZEBOX, WS_SYSMENU, WS_THICKFRAME,
    };
    WS_CAPTION | WS_THICKFRAME | WS_SYSMENU | WS_MINIMIZEBOX | WS_MAXIMIZEBOX | WS_MAXIMIZE
}

fn cover_style_ok(style: u32) -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::WS_POPUP;
    (style & WS_POPUP) != 0 && (style & cover_chrome_bits()) == 0
}

fn host_style(hwnd: Hwnd) -> u32 {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetWindowLongPtrW, GWL_STYLE};
    unsafe { GetWindowLongPtrW(hwnd, GWL_STYLE) as u32 }
}

fn rect_matches_work_area(hwnd: Hwnd) -> bool {
    let (x, y, w, h) = work_area();
    let (wx, wy, ww, wh) = window_rect(hwnd);
    (wx - x).abs() <= 1 && (wy - y).abs() <= 1 && (ww - w).abs() <= 1 && (wh - h).abs() <= 1
}

fn host_already_frameless(hwnd: Hwnd) -> bool {
    cover_style_ok(host_style(hwnd)) && rect_matches_work_area(hwnd)
}

thread_local! {
    static IN_COVER_LAYOUT: Cell<bool> = const { Cell::new(false) };
}

struct CoverLayoutGuard;

impl Drop for CoverLayoutGuard {
    fn drop(&mut self) {
        IN_COVER_LAYOUT.set(false);
    }
}

fn class_is_shell_desktop(class: &str) -> bool {
    class.eq_ignore_ascii_case("Progman") || class.eq_ignore_ascii_case("WorkerW")
}

fn window_is_shell_desktop(hwnd: Hwnd) -> bool {
    !hwnd.is_null() && class_is_shell_desktop(&window_class(hwnd))
}

fn is_cover_peer(hwnd: Hwnd) -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetWindowLongPtrW, GWLP_USERDATA};
    if hwnd.is_null() || window_class(hwnd) != "StarDustHost" {
        return false;
    }
    unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) != 0 }
}

fn collect_shell_desktops() -> Vec<Hwnd> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{FindWindowExW, FindWindowW};
    let mut out = Vec::new();
    unsafe {
        let progman = FindWindowW(wide("Progman").as_ptr(), std::ptr::null());
        if !progman.is_null() {
            out.push(progman);
        }
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
            out.push(worker);
        }
    }
    out
}

fn z_is_above(a: Hwnd, b: Hwnd) -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetWindow, GW_HWNDPREV};
    if a.is_null() || b.is_null() || a == b {
        return false;
    }
    unsafe {
        let mut w = b;
        loop {
            w = GetWindow(w, GW_HWNDPREV);
            if w.is_null() {
                return false;
            }
            if w == a {
                return true;
            }
        }
    }
}

fn highest_shell_desktop() -> Hwnd {
    let shells = collect_shell_desktops();
    let mut best: Hwnd = std::ptr::null_mut();
    for s in shells {
        if best.is_null() || z_is_above(s, best) {
            best = s;
        }
    }
    best
}

fn shell_is_above(hwnd: Hwnd) -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetWindow, GW_HWNDPREV};
    unsafe {
        let mut w = hwnd;
        loop {
            w = GetWindow(w, GW_HWNDPREV);
            if w.is_null() {
                return false;
            }
            if window_is_shell_desktop(w) {
                return true;
            }
        }
    }
}

/// Change cover z-order only when StarDust is foreground (startup / user clicked
/// the cosmos) or Progman/WorkerW is above the cover (wallpaper hiding cosmos).
/// Never restack while Chrome is foreground and Cursor is being taskbar-raised:
/// that `SetWindowPos` parks the cover under Chrome and leaves Cursor below it.
fn cover_may_change_z_order(self_foreground: bool, shell_above: bool) -> bool {
    self_foreground || shell_above
}

/// `WM_WINDOWPOSCHANGING` must not rewrite `hwndInsertAfter` (to `HWND_BOTTOM`
/// or to Chrome). Neighbor z-shuffles from a taskbar raise share this message;
/// any cover `SetWindowPos` there cancels Cursor coming in front of Chrome.
fn cover_changing_should_lock_z(hide: bool, already_no_zorder: bool) -> bool {
    !hide && !already_no_zorder
}

fn cover_may_change_z(hwnd: Hwnd) -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
    if hwnd.is_null() {
        return false;
    }
    let fg = unsafe { GetForegroundWindow() };
    let self_fg = fg.is_null() || fg == hwnd || is_cover_peer(fg);
    cover_may_change_z_order(self_fg, shell_is_above(hwnd))
}

/// Place just above the shell desktop. `hwndInsertAfter` may be a user app when
/// that app is already the window above Progman/WorkerW — only callers that
/// passed `cover_may_change_z` may use this (not `WM_WINDOWPOSCHANGED` pin).
fn cover_above_wallpaper_insert_after(hwnd: Hwnd) -> Hwnd {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetWindow, GW_HWNDPREV, HWND_BOTTOM};
    let shell = highest_shell_desktop();
    if shell.is_null() {
        return HWND_BOTTOM;
    }
    unsafe {
        let prev = GetWindow(shell, GW_HWNDPREV);
        if prev.is_null() || prev == hwnd {
            HWND_BOTTOM
        } else {
            prev
        }
    }
}

fn cover_set_window_pos(hwnd: Hwnd, x: i32, y: i32, w: i32, h: i32, extra_flags: u32) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SetWindowPos, HWND_BOTTOM, SWP_NOACTIVATE, SWP_NOOWNERZORDER, SWP_NOZORDER,
    };
    let mut flags = extra_flags | SWP_NOACTIVATE | SWP_NOOWNERZORDER;
    let insert = if cover_may_change_z(hwnd) {
        cover_above_wallpaper_insert_after(hwnd)
    } else {
        flags |= SWP_NOZORDER;
        HWND_BOTTOM
    };
    unsafe {
        let _ = SetWindowPos(hwnd, insert, x, y, w, h, flags);
    }
}

/// Refuse every z-order rewrite on the cover, including `HWND_TOP` and
/// insert-after Chrome. Never `HWND_BOTTOM` Progman/WorkerW.
pub(super) fn apply_cover_windowpos_changing(hwnd: Hwnd, lparam: isize) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, GWLP_USERDATA, SWP_HIDEWINDOW, SWP_NOZORDER, WINDOWPOS,
    };
    if hwnd.is_null() || lparam == 0 {
        return;
    }
    if unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } == 0 {
        return;
    }
    let wp = lparam as *mut WINDOWPOS;
    unsafe {
        let hide = (*wp).flags & SWP_HIDEWINDOW != 0;
        let already = (*wp).flags & SWP_NOZORDER != 0;
        if cover_changing_should_lock_z(hide, already) {
            (*wp).flags |= SWP_NOZORDER;
        }
    }
}

fn layout_host_desktop(hwnd: Hwnd, force_frame: bool) {
    use windows_sys::Win32::Graphics::Gdi::SetWindowRgn;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, IsIconic, IsWindowVisible, SetWindowLongPtrW, ShowWindow, GWL_STYLE,
        SWP_FRAMECHANGED, SW_RESTORE, SW_SHOWNA, WS_MINIMIZE, WS_POPUP, WS_VISIBLE,
    };
    if IN_COVER_LAYOUT.get() {
        return;
    }

    let cur_style = host_style(hwnd);
    let iconic = unsafe { IsIconic(hwnd) } != 0;
    let foreground = unsafe { GetForegroundWindow() } == hwnd;
    let already_cover =
        cover_style_ok(cur_style) && (cur_style & WS_MINIMIZE) == 0 && rect_matches_work_area(hwnd);
    // Do not SetWindowPos / pin on WM_WINDOWPOSCHANGED when the cover is already
    // a work-area popup — that races Chrome vs taskbar-raised Cursor.
    if already_cover && !force_frame {
        return;
    }

    IN_COVER_LAYOUT.set(true);
    let _guard = CoverLayoutGuard;

    set_desktop_host_flag(hwnd, true);
    set_ex_appwindow(hwnd, false);
    dwm_disable_nc(hwnd);

    let visible = unsafe { IsWindowVisible(hwnd) } != 0;
    // Do not ShowWindow/SW_RESTORE an iconic host that is not foreground — that
    // steals exclusive fullscreen from another app.
    let keep_minimized = iconic && !foreground;
    let want_style = if keep_minimized {
        WS_POPUP | WS_VISIBLE | WS_MINIMIZE
    } else {
        WS_POPUP | WS_VISIBLE
    };

    unsafe {
        SetWindowLongPtrW(hwnd, GWL_STYLE, want_style as isize);
        let _ = SetWindowRgn(hwnd, std::ptr::null_mut(), 1);
    }
    if keep_minimized {
        return;
    }

    let (x, y, w, h) = work_area();
    cover_set_window_pos(hwnd, x, y, w, h, SWP_FRAMECHANGED);
    if iconic && foreground {
        unsafe {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        cover_set_window_pos(hwnd, x, y, w, h, SWP_FRAMECHANGED);
    } else if visible {
        unsafe {
            let _ = ShowWindow(hwnd, SW_SHOWNA);
        }
    }
}

fn apply_region_clip(hwnd: Hwnd) {
    use windows_sys::Win32::Graphics::Gdi::{CreateRectRgn, SetWindowRgn};
    use windows_sys::Win32::UI::WindowsAndMessaging::{ShowWindow, SWP_FRAMECHANGED, SW_SHOW};
    if clip_already_ok(hwnd) {
        return;
    }
    let (tx, ty, tw, th, fl, cap, w, h) = clip_target(hwnd);
    dwm_hide_chrome(hwnd);
    cover_set_window_pos(hwnd, tx, ty, tw, th, SWP_FRAMECHANGED);
    unsafe {
        let rgn = CreateRectRgn(fl, cap, fl + w, cap + h);
        let _ = SetWindowRgn(hwnd, rgn, 1);
        let _ = ShowWindow(hwnd, SW_SHOW);
    }
}

#[allow(dead_code)]
pub(super) fn already_frameless() -> bool {
    let hosts = visible_stardust_hosts();
    if !hosts.is_empty() {
        let flags: Vec<bool> = hosts.iter().copied().map(host_already_frameless).collect();
        return hosts_all_cover(&flags);
    }
    let hwnd = find_stardust_hwnd();
    if hwnd.is_null() {
        return false;
    }
    clip_already_ok(hwnd)
}

fn layout_all_host_covers(force_frame: bool) {
    let hosts = stardust_hosts();
    if !hosts.is_empty() {
        for hwnd in hosts {
            layout_host_desktop(hwnd, force_frame);
        }
        return;
    }
    let hwnd = find_stardust_hwnd();
    if hwnd.is_null() {
        return;
    }
    apply_region_clip(hwnd);
}

pub(super) fn keep_frameless() {
    layout_all_host_covers(true);
}

/// Re-cover only hosts whose style/rect is not already a clean work-area popup.
/// Does not pin z-order (`WM_WINDOWPOSCHANGED` neighbor shuffles must not
/// `SetWindowPos` the cover).
pub(super) fn keep_cover_if_wrong() {
    layout_all_host_covers(false);
}

pub(super) fn place_stardust_window() {
    close_toplevel_guard_window(false);
    sweep_host_tooltips(false);
    release_cover_hosts();
    let hosts = stardust_hosts();
    if !hosts.is_empty() {
        for hwnd in hosts {
            layout_host_desktop(hwnd, true);
        }
        note_cover(2, "desktop-surface webview-host");
        return;
    }
    let hwnd = find_stardust_hwnd();
    if hwnd.is_null() {
        return;
    }
    apply_region_clip(hwnd);
    note_cover(1, "desktop-surface cover-clip");
}

fn float_one_host(hwnd: Hwnd) {
    use windows_sys::Win32::Graphics::Gdi::SetWindowRgn;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SetWindowLongPtrW, SetWindowPos, GWL_STYLE, HWND_TOP, SWP_FRAMECHANGED, SWP_SHOWWINDOW,
        WS_OVERLAPPEDWINDOW, WS_VISIBLE,
    };
    unsafe {
        let _ = SetWindowRgn(hwnd, std::ptr::null_mut(), 1);
        SetWindowLongPtrW(hwnd, GWL_STYLE, (WS_OVERLAPPEDWINDOW | WS_VISIBLE) as isize);
        set_desktop_host_flag(hwnd, false);
        set_ex_appwindow(hwnd, true);
        dwm_restore_chrome(hwnd);
        let (x, y, wa_w, wa_h) = work_area();
        let w = 1100.min(wa_w);
        let h = 720.min(wa_h);
        let nx = x + (wa_w - w) / 2;
        let ny = y + (wa_h - h) / 2;
        let _ = SetWindowPos(hwnd, HWND_TOP, nx, ny, w, h, SWP_FRAMECHANGED | SWP_SHOWWINDOW);
    }
}

pub(super) fn float_stardust_window() {
    reset_clip_insets();
    release_cover_hosts();
    let hosts = stardust_hosts();
    if hosts.is_empty() {
        let hwnd = find_stardust_hwnd();
        if hwnd.is_null() {
            sweep_host_tooltips(true);
            return;
        }
        float_one_host(hwnd);
        sweep_host_tooltips(true);
        return;
    }
    for hwnd in hosts {
        float_one_host(hwnd);
    }
    sweep_host_tooltips(true);
}

#[cfg(test)]
mod cover_logic_tests {
    use super::{cover_style_ok, hosts_all_cover};

    #[test]
    fn first_frameless_must_not_skip_other_captioned_hosts() {
        assert!(
            !hosts_all_cover(&[true, false]),
            "a frameless first hwnd must not skip a captioned sibling"
        );
        assert!(!hosts_all_cover(&[false, true]));
        assert!(!hosts_all_cover(&[true, true, false]));
        assert!(!hosts_all_cover(&[]));
        assert!(!hosts_all_cover(&[false]));
        assert!(hosts_all_cover(&[true]));
        assert!(hosts_all_cover(&[true, true]));
    }

    #[test]
    fn progman_and_workerw_are_shell_desktop() {
        assert!(super::class_is_shell_desktop("Progman"));
        assert!(super::class_is_shell_desktop("WorkerW"));
        assert!(super::class_is_shell_desktop("workerw"));
        assert!(!super::class_is_shell_desktop("StarDustHost"));
        assert!(!super::class_is_shell_desktop("Shell_TrayWnd"));
    }

    #[test]
    fn cover_does_not_restack_during_other_app_raise() {
        assert!(
            !super::cover_may_change_z_order(false, false),
            "Chrome foreground + Cursor taskbar raise must not SetWindowPos the cover"
        );
        assert!(
            super::cover_may_change_z_order(true, false),
            "StarDust foreground (startup / click cosmos) may place above wallpaper"
        );
        assert!(
            super::cover_may_change_z_order(false, true),
            "Progman/WorkerW above the cover may place above wallpaper"
        );
    }

    #[test]
    fn cover_windowpos_changing_locks_z_and_never_inserts_after_chrome() {
        assert!(
            super::cover_changing_should_lock_z(false, false),
            "neighbor shuffle / HWND_TOP must become SWP_NOZORDER"
        );
        assert!(
            !super::cover_changing_should_lock_z(true, false),
            "hide may keep its z-order flags"
        );
        assert!(
            !super::cover_changing_should_lock_z(false, true),
            "already SWP_NOZORDER stays as-is"
        );
    }

    #[test]
    fn popup_with_caption_is_not_a_cover() {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            WS_CAPTION, WS_OVERLAPPEDWINDOW, WS_POPUP, WS_THICKFRAME, WS_VISIBLE,
        };
        assert!(cover_style_ok(WS_POPUP | WS_VISIBLE));
        assert!(
            !cover_style_ok(WS_POPUP | WS_CAPTION | WS_VISIBLE),
            "WS_POPUP plus leftover caption is still OS chrome"
        );
        assert!(!cover_style_ok(WS_POPUP | WS_THICKFRAME | WS_VISIBLE));
        assert!(!cover_style_ok(WS_OVERLAPPEDWINDOW | WS_VISIBLE));
        assert!(!cover_style_ok(WS_VISIBLE));
    }
}
