//! Native WebView2 host so StarDust is an app, not an Edge `--app=` window.
//! Browser accelerators, page zoom, swipe-nav, and the Edge context menu stay off.
//!
//! Uses windows-sys + a classic COM vtable (not webview2-com `#[implement]`).
//! GNU rustc + windows-rs COM callbacks jump to a wild RIP (0xc0000005 / BEX64).

use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::io;
use std::mem;
use std::path::Path;
use std::ptr;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HWND, LPARAM, LRESULT, RECT, WPARAM,
};
use windows_sys::Win32::Graphics::Gdi::UpdateWindow;
use windows_sys::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
use windows_sys::Win32::System::LibraryLoader::{
    GetModuleFileNameW, GetModuleHandleW, GetProcAddress, LoadLibraryW,
};
use windows_sys::Win32::System::Threading::CreateMutexW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetClientRect, GetMessageW,
    GetWindowLongPtrW, KillTimer, LoadCursorW, LoadImageW, PeekMessageW, PostQuitMessage,
    RegisterClassW, SetTimer, SetWindowLongPtrW, ShowWindow, SystemParametersInfoW,
    TranslateMessage, CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, GWLP_USERDATA, HTCLIENT, IDC_ARROW,
    IMAGE_ICON, LR_DEFAULTSIZE, LR_LOADFROMFILE, LR_SHARED, MSG, PM_REMOVE, SC_RESTORE,
    SIZE_MAXIMIZED, SIZE_MINIMIZED, SIZE_RESTORED, SPI_GETWORKAREA, SPI_SETWORKAREA, SW_SHOW,
    WM_ACTIVATE, WM_DESTROY, WM_DISPLAYCHANGE, WM_EXITSIZEMOVE, WM_GETMINMAXINFO, WM_NCHITTEST,
    WM_SETTINGCHANGE, WM_SHOWWINDOW, WM_SIZE, WM_SYSCOMMAND, WM_TIMER, WM_WINDOWPOSCHANGED,
    WM_WINDOWPOSCHANGING, WNDCLASSW, WS_EX_APPWINDOW, WS_OVERLAPPEDWINDOW, WS_POPUP, WS_VISIBLE,
};

pub const HOST_CLASS: &str = "StarDustHost";

const S_OK: i32 = 0;
const E_NOINTERFACE: i32 = -2147467262; // 0x80004002
const E_POINTER: i32 = -2147467261; // 0x80004003
const ERROR_CLASS_ALREADY_EXISTS: u32 = 1410;

const IID_IUNKNOWN: Guid = guid(0x0000_0000_0000_0000_c000_0000_0000_0046);
const IID_ENV_HANDLER: Guid = guid(0x4e8a_3389_c9d8_4bd2_b6b5_124f_ee6c_c14d);
const IID_CTRL_HANDLER: Guid = guid(0x6c48_19f3_c9b7_4260_8127_c9f5_bde7_f68c);
const IID_SETTINGS3: Guid = guid(0xfdb5_ab74_af33_4854_84f0_0a63_1deb_5eba);
const IID_SETTINGS5: Guid = guid(0x183e_7052_1d03_43a0_ab99_98e0_43b6_6b39);
const IID_SETTINGS6: Guid = guid(0x11cb_3acd_9bc8_43b8_83bf_f407_5371_4f87);

const MOVE_FOCUS_PROGRAMMATIC: i32 = 0;
const NAV_TIMER_ID: usize = 0xD601;

thread_local! {
    static CONTROLLER: Cell<*mut c_void> = const { Cell::new(ptr::null_mut()) };
    static NAV_URL: RefCell<String> = const { RefCell::new(String::new()) };
    static NAV_LOADED: Cell<bool> = const { Cell::new(false) };
    static NAV_BACKOFF_MS: Cell<u32> = const { Cell::new(250) };
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Guid {
    data1: u32,
    data2: u16,
    data3: u16,
    data4: [u8; 8],
}

const fn guid(v: u128) -> Guid {
    Guid {
        data1: (v >> 96) as u32,
        data2: (v >> 80) as u16,
        data3: (v >> 64) as u16,
        data4: [
            (v >> 56) as u8,
            (v >> 48) as u8,
            (v >> 40) as u8,
            (v >> 32) as u8,
            (v >> 24) as u8,
            (v >> 16) as u8,
            (v >> 8) as u8,
            v as u8,
        ],
    }
}

fn guid_eq(a: &Guid, b: &Guid) -> bool {
    a.data1 == b.data1 && a.data2 == b.data2 && a.data3 == b.data3 && a.data4 == b.data4
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn io_err(e: impl std::fmt::Display) -> io::Error {
    io::Error::other(e.to_string())
}

fn hr_err(what: &str, hr: i32) -> io::Error {
    io_err(format!("{what} HRESULT {hr:#010x}"))
}

fn work_area() -> (i32, i32, i32, i32) {
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

unsafe fn vtbl(obj: *mut c_void) -> *const *const c_void {
    *(obj as *const *const *const c_void)
}

unsafe fn call0(obj: *mut c_void, slot: usize) -> i32 {
    let f: unsafe extern "system" fn(*mut c_void) -> i32 = mem::transmute(*vtbl(obj).add(slot));
    f(obj)
}

unsafe fn add_ref(obj: *mut c_void) {
    if !obj.is_null() {
        let _ = call0(obj, 1);
    }
}

unsafe fn release(obj: *mut c_void) {
    if !obj.is_null() {
        let _ = call0(obj, 2);
    }
}

unsafe fn query_interface(obj: *mut c_void, iid: &Guid) -> *mut c_void {
    let f: unsafe extern "system" fn(*mut c_void, *const Guid, *mut *mut c_void) -> i32 =
        mem::transmute(*vtbl(obj).add(0));
    let mut out = ptr::null_mut();
    if f(obj, iid, &mut out) >= 0 {
        out
    } else {
        ptr::null_mut()
    }
}

unsafe fn put_bool(obj: *mut c_void, slot: usize, value: i32) -> i32 {
    let f: unsafe extern "system" fn(*mut c_void, i32) -> i32 =
        mem::transmute(*vtbl(obj).add(slot));
    f(obj, value)
}

type Completed = Result<*mut c_void, i32>;

#[repr(C)]
struct CompletedVtbl {
    query_interface:
        unsafe extern "system" fn(*mut CompletedHandler, *const Guid, *mut *mut c_void) -> i32,
    add_ref: unsafe extern "system" fn(*mut CompletedHandler) -> u32,
    release: unsafe extern "system" fn(*mut CompletedHandler) -> u32,
    invoke: unsafe extern "system" fn(*mut CompletedHandler, i32, *mut c_void) -> i32,
}

#[repr(C)]
struct CompletedHandler {
    vtbl: *const CompletedVtbl,
    refs: AtomicI32,
    iid: Guid,
    tx: Option<mpsc::Sender<Completed>>,
}

unsafe extern "system" fn handler_qi(
    this: *mut CompletedHandler,
    iid: *const Guid,
    out: *mut *mut c_void,
) -> i32 {
    if this.is_null() || iid.is_null() || out.is_null() {
        return E_POINTER;
    }
    let h = &*this;
    if guid_eq(&*iid, &IID_IUNKNOWN) || guid_eq(&*iid, &h.iid) {
        *out = this.cast();
        handler_add_ref(this);
        S_OK
    } else {
        *out = ptr::null_mut();
        E_NOINTERFACE
    }
}

unsafe extern "system" fn handler_add_ref(this: *mut CompletedHandler) -> u32 {
    (*this).refs.fetch_add(1, Ordering::Relaxed) as u32 + 1
}

unsafe extern "system" fn handler_release(this: *mut CompletedHandler) -> u32 {
    let prev = (*this).refs.fetch_sub(1, Ordering::Release);
    if prev == 1 {
        drop(Box::from_raw(this));
        0
    } else {
        (prev - 1) as u32
    }
}

unsafe extern "system" fn handler_invoke(
    this: *mut CompletedHandler,
    error: i32,
    created: *mut c_void,
) -> i32 {
    if this.is_null() {
        return E_POINTER;
    }
    let msg = if error < 0 {
        Err(error)
    } else if created.is_null() {
        Err(E_POINTER)
    } else {
        add_ref(created);
        Ok(created)
    };
    if let Some(tx) = (*this).tx.take() {
        let _ = tx.send(msg);
    } else if let Ok(p) = msg {
        release(p);
    }
    S_OK
}

static ENV_VTBL: CompletedVtbl = CompletedVtbl {
    query_interface: handler_qi,
    add_ref: handler_add_ref,
    release: handler_release,
    invoke: handler_invoke,
};

static CTRL_VTBL: CompletedVtbl = CompletedVtbl {
    query_interface: handler_qi,
    add_ref: handler_add_ref,
    release: handler_release,
    invoke: handler_invoke,
};

fn make_handler(
    iid: Guid,
    vtbl: &'static CompletedVtbl,
) -> (*mut CompletedHandler, mpsc::Receiver<Completed>) {
    let (tx, rx) = mpsc::channel();
    let handler = Box::new(CompletedHandler {
        vtbl,
        refs: AtomicI32::new(1),
        iid,
        tx: Some(tx),
    });
    (Box::into_raw(handler), rx)
}

fn wait_with_pump(rx: mpsc::Receiver<Completed>) -> io::Result<*mut c_void> {
    let mut msg = unsafe { mem::zeroed::<MSG>() };
    loop {
        if let Ok(result) = rx.try_recv() {
            return result.map_err(|hr| hr_err("WebView2 callback", hr));
        }
        unsafe {
            match GetMessageW(&mut msg, ptr::null_mut(), 0, 0) {
                -1 => return Err(io_err("GetMessageW failed")),
                0 => return Err(io_err("WebView2 canceled")),
                _ => {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
        }
    }
}

fn apply_bounds(controller: *mut c_void, hwnd: HWND) {
    if fs_viz::shell::ime_blocks_bounds_focus_script(fs_viz::shell::ime_composing()) {
        return;
    }
    if fs_viz::shell::cover_skip_webview_bounds(hwnd) {
        return;
    }
    if controller.is_null() {
        return;
    }
    let mut r = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    unsafe {
        if GetClientRect(hwnd, &mut r) == 0 {
            return;
        }
        let f: unsafe extern "system" fn(*mut c_void, *const RECT) -> i32 =
            mem::transmute(*vtbl(controller).add(6));
        let _ = f(controller, &r);
    }
    if is_desktop_host(hwnd) {
        fs_viz::shell::apply_cover_non_rude(hwnd);
    }
}

fn is_desktop_host(hwnd: HWND) -> bool {
    unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) != 0 }
}

fn is_work_area_change(wparam: WPARAM, lparam: LPARAM) -> bool {
    if wparam == SPI_SETWORKAREA as usize {
        return true;
    }
    if lparam == 0 {
        return false;
    }
    const WANT: [u16; 9] = [
        b'W' as u16,
        b'o' as u16,
        b'r' as u16,
        b'k' as u16,
        b'A' as u16,
        b'r' as u16,
        b'e' as u16,
        b'a' as u16,
        0,
    ];
    unsafe {
        let p = lparam as *const u16;
        for (i, want) in WANT.iter().enumerate() {
            if *p.add(i) != *want {
                return false;
            }
        }
    }
    true
}

/// ICoreWebView2Controller: put_IsVisible=4, put_Bounds=6, MoveFocus=12,
/// NotifyParentWindowPositionChanged=23, get_CoreWebView2=25.
/// ICoreWebView2: ExecuteScript=29.
fn resume_webview(hwnd: HWND) {
    CONTROLLER.with(|slot| {
        let controller = slot.get();
        if controller.is_null() {
            return;
        }
        unsafe {
            let _ = put_bool(controller, 4, 1);
        }
        if fs_viz::shell::ime_blocks_bounds_focus_script(fs_viz::shell::ime_composing()) {
            return;
        }
        apply_bounds(controller, hwnd);
        unsafe {
            let _ = call0(controller, 23);
            let move_focus: unsafe extern "system" fn(*mut c_void, i32) -> i32 =
                mem::transmute(*vtbl(controller).add(12));
            let _ = move_focus(controller, MOVE_FOCUS_PROGRAMMATIC);
            let get_webview: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> i32 =
                mem::transmute(*vtbl(controller).add(25));
            let mut webview = ptr::null_mut();
            if get_webview(controller, &mut webview) >= 0 && !webview.is_null() {
                let js = wide(
                    "try{window.SD&&SD.app&&SD.app.resumeFromHost&&SD.app.resumeFromHost()}catch(e){}",
                );
                let exec: unsafe extern "system" fn(*mut c_void, *const u16, *mut c_void) -> i32 =
                    mem::transmute(*vtbl(webview).add(29));
                let _ = exec(webview, js.as_ptr(), ptr::null_mut());
                release(webview);
            }
        }
    });
}

fn on_host_shown(hwnd: HWND) {
    // Restore from UiMode, not GWLP_USERDATA: exclusive fullscreen can restyle
    // the hwnd to overlapped while the in-app chrome is still StarDust UI.
    // Do not SetWindowPos z-order when the cover is already a work-area popup.
    // IME/RIME activate must not restyle or resume WebView (flash). Other-app
    // FG must not restyle (leave exclusive fullscreen alone).
    if fs_viz::shell::ime_composing() || fs_viz::shell::cover_ime_ui_foreground() {
        return;
    }
    if !fs_viz::shell::cover_other_user_app_foreground() {
        fs_viz::shell::restore_stardust_cover_if_wrong();
    }
    resume_webview(hwnd);
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_NCHITTEST => {
            if is_desktop_host(hwnd) {
                HTCLIENT as LRESULT
            } else {
                unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
            }
        }
        WM_ACTIVATE => {
            if is_desktop_host(hwnd) {
                fs_viz::shell::apply_cover_non_rude(hwnd);
            }
            if (wparam as u32) & 0xFFFF != 0 {
                on_host_shown(hwnd);
            }
            let result = unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
            // DefWindowProc/SetForegroundWindow can raise the host over docked
            // HWNDs (sash click). Put docks back on top without activating them.
            if is_desktop_host(hwnd) && (wparam as u32) & 0xFFFF != 0 {
                fs_viz::shell::raise_user_dock_hwnds();
            }
            result
        }
        WM_SHOWWINDOW => {
            if is_desktop_host(hwnd)
                && fs_viz::shell::cover_refuse_host_show_hide(true, wparam != 0)
            {
                fs_viz::shell::cover_stay_visible_from_show_desktop(hwnd, true);
                return 0;
            }
            if wparam != 0 {
                on_host_shown(hwnd);
            }
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        WM_GETMINMAXINFO => {
            let result = unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
            if is_desktop_host(hwnd) {
                fs_viz::shell::pin_stardust_cover_minmax(lparam);
            }
            result
        }
        WM_WINDOWPOSCHANGING => {
            if is_desktop_host(hwnd) {
                let _ = fs_viz::shell::pin_stardust_cover_changing(hwnd, lparam);
            }
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        WM_WINDOWPOSCHANGED => {
            if is_desktop_host(hwnd) {
                fs_viz::shell::cover_stay_visible_from_show_desktop(hwnd, false);
            }
            if !fs_viz::shell::cover_skip_restore_if_wrong() {
                fs_viz::shell::restore_stardust_cover_if_wrong();
            }
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        WM_EXITSIZEMOVE => {
            fs_viz::shell::restore_stardust_cover();
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        WM_SYSCOMMAND => {
            let cmd = (wparam as u32) & 0xFFF0;
            if fs_viz::shell::cover_swallow_host_minimize(is_desktop_host(hwnd), cmd) {
                return 0;
            }
            let result = unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
            if cmd == SC_RESTORE {
                on_host_shown(hwnd);
            }
            result
        }
        WM_DISPLAYCHANGE => {
            on_host_shown(hwnd);
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        msg if msg == fs_viz::shell::WM_POWERBROADCAST => {
            if fs_viz::shell::is_power_resume(wparam) {
                // Sleep/lid: refresh heartbeat + rebuild hold so the guard does
                // not yellow-failover, then force cover restyle and resume WebView2.
                fs_viz::shell::touch_heartbeat();
                fs_viz::shell::begin_rebuild_hold();
                fs_viz::shell::restore_stardust_cover();
                resume_webview(hwnd);
            }
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        msg if msg != 0 && msg == fs_viz::shell::wm_open_cursor() => {
            fs_viz::shell::open_cursor_from_view();
            0
        }
        WM_SETTINGCHANGE => {
            if is_work_area_change(wparam, lparam) {
                on_host_shown(hwnd);
            }
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        WM_SIZE => {
            if wparam == SIZE_MINIMIZED as usize {
                // Do not put_Bounds on a 0×0 iconic client (torn split / black pane).
                fs_viz::shell::cover_stay_visible_from_show_desktop(hwnd, true);
                fs_viz::shell::restore_stardust_cover();
                resume_webview(hwnd);
                return 0;
            }
            if wparam == SIZE_RESTORED as usize || wparam == SIZE_MAXIMIZED as usize {
                fs_viz::shell::restore_stardust_cover_if_wrong();
            }
            if !fs_viz::shell::ime_blocks_bounds_focus_script(fs_viz::shell::ime_composing()) {
                CONTROLLER.with(|slot| apply_bounds(slot.get(), hwnd));
            }
            0
        }
        WM_TIMER => {
            if wparam == NAV_TIMER_ID {
                on_nav_timer(hwnd);
                0
            } else {
                unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
            }
        }
        WM_DESTROY => {
            CONTROLLER.with(|slot| {
                let p = slot.replace(ptr::null_mut());
                unsafe { release(p) };
            });
            fs_viz::shell::uninstall_cover_cbt_hook();
            fs_viz::shell::sweep_host_tooltips_on_host_destroy();
            unsafe { PostQuitMessage(0) };
            0
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

fn icon_wide(path: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    path.as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

fn load_icon_from(path: &Path) -> windows_sys::Win32::UI::WindowsAndMessaging::HICON {
    if !path.is_file() {
        return ptr::null_mut();
    }
    let w = icon_wide(path);
    unsafe {
        LoadImageW(
            ptr::null_mut(),
            w.as_ptr(),
            IMAGE_ICON,
            0,
            0,
            LR_LOADFROMFILE | LR_DEFAULTSIZE,
        )
    }
}

fn load_icon_from_pe() -> windows_sys::Win32::UI::WindowsAndMessaging::HICON {
    unsafe {
        LoadImageW(
            GetModuleHandleW(ptr::null()),
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

/// Luna mark: PE resource, then exe-dir `stardust.ico`, then crate assets.
fn load_stardust_icon() -> windows_sys::Win32::UI::WindowsAndMessaging::HICON {
    use std::sync::atomic::{AtomicIsize, Ordering};
    use std::sync::Once;
    static ONCE: Once = Once::new();
    static ICON: AtomicIsize = AtomicIsize::new(0);
    ONCE.call_once(|| {
        let mut h = load_icon_from_pe();
        if h.is_null() {
            let mut buf = [0u16; 1024];
            let n =
                unsafe { GetModuleFileNameW(ptr::null_mut(), buf.as_mut_ptr(), buf.len() as u32) };
            if n > 0 && (n as usize) < buf.len() {
                let exe = String::from_utf16_lossy(&buf[..n as usize]);
                if let Some(dir) = Path::new(&exe).parent() {
                    h = load_icon_from(&dir.join("stardust.ico"));
                }
            }
        }
        if h.is_null() {
            h = load_icon_from(
                &Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("assets")
                    .join("stardust.ico"),
            );
        }
        ICON.store(h as isize, Ordering::Release);
    });
    ICON.load(Ordering::Acquire) as _
}

fn register_class() -> io::Result<()> {
    let class = wide(HOST_CLASS);
    let cursor = unsafe { LoadCursorW(ptr::null_mut(), IDC_ARROW) };
    let hicon = load_stardust_icon();
    let wc = WNDCLASSW {
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(wndproc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: unsafe { GetModuleHandleW(ptr::null()) },
        hIcon: hicon,
        hCursor: cursor,
        hbrBackground: ptr::null_mut(),
        lpszMenuName: ptr::null(),
        lpszClassName: class.as_ptr(),
    };
    let atom = unsafe { RegisterClassW(&wc) };
    if atom == 0 {
        let err = unsafe { windows_sys::Win32::Foundation::GetLastError() };
        if err == ERROR_CLASS_ALREADY_EXISTS {
            return Ok(());
        }
        return Err(io_err(format!("RegisterClassW {err}")));
    }
    Ok(())
}

fn create_hwnd(stardust_desktop: bool) -> io::Result<HWND> {
    register_class()?;
    let class = wide(HOST_CLASS);
    let title = wide("StarDust");
    let (x, y, w, h, style) = if stardust_desktop {
        let (x, y, w, h) = work_area();
        (x, y, w, h, WS_POPUP)
    } else {
        (
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            1100,
            720,
            WS_OVERLAPPEDWINDOW | WS_VISIBLE,
        )
    };
    let ex_style = if stardust_desktop { 0 } else { WS_EX_APPWINDOW };
    let hwnd = unsafe {
        CreateWindowExW(
            ex_style,
            class.as_ptr(),
            title.as_ptr(),
            style,
            x,
            y,
            w,
            h,
            ptr::null_mut(),
            ptr::null_mut(),
            GetModuleHandleW(ptr::null()),
            ptr::null(),
        )
    };
    if hwnd.is_null() {
        return Err(io_err("CreateWindowExW failed"));
    }
    fs_viz::shell::apply_host_window_icons(hwnd, !stardust_desktop);
    if stardust_desktop {
        unsafe {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 1);
        }
        dwm_disable_nc(hwnd);
        fs_viz::shell::install_cover_cbt_hook();
        fs_viz::shell::apply_cover_non_rude(hwnd);
    }
    Ok(hwnd)
}

fn dwm_disable_nc(hwnd: HWND) {
    const DWMWA_NCRENDERING_POLICY: u32 = 2;
    const DWMNCRP_DISABLED: u32 = 1;
    let value = DWMNCRP_DISABLED;
    unsafe {
        let _ = windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute(
            hwnd,
            DWMWA_NCRENDERING_POLICY,
            (&value as *const u32).cast(),
            4,
        );
    }
}

fn load_create_env(
) -> io::Result<unsafe extern "system" fn(*const u16, *const u16, *mut c_void, *mut c_void) -> i32>
{
    let mut candidates = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("WebView2Loader.dll"));
        }
    }
    candidates.push(Path::new("WebView2Loader.dll").to_path_buf());
    let mut last = io_err("WebView2Loader.dll not found");
    for path in candidates {
        let w = wide(&path.to_string_lossy());
        let module = unsafe { LoadLibraryW(w.as_ptr()) };
        if module.is_null() {
            last = io_err(format!("LoadLibrary {}", path.display()));
            continue;
        }
        let proc =
            unsafe { GetProcAddress(module, b"CreateCoreWebView2EnvironmentWithOptions\0".as_ptr()) };
        if proc.is_none() {
            last = io_err("CreateCoreWebView2EnvironmentWithOptions missing");
            continue;
        }
        return Ok(unsafe {
            mem::transmute::<
                Option<unsafe extern "system" fn() -> isize>,
                unsafe extern "system" fn(*const u16, *const u16, *mut c_void, *mut c_void) -> i32,
            >(proc)
        });
    }
    Err(last)
}

fn apply_settings(webview: *mut c_void) {
    type GetObj = unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> i32;
    let get_settings: GetObj = unsafe { mem::transmute(*vtbl(webview).add(3)) };
    let mut settings = ptr::null_mut();
    if unsafe { get_settings(webview, &mut settings) } < 0 || settings.is_null() {
        return;
    }
    unsafe {
        let _ = put_bool(settings, 10, 0); // SetIsStatusBarEnabled
        let _ = put_bool(settings, 12, 0); // SetAreDevToolsEnabled
        let _ = put_bool(settings, 14, 0); // SetAreDefaultContextMenusEnabled
        let _ = put_bool(settings, 18, 0); // SetIsZoomControlEnabled
        if let Some(s3) = non_null(query_interface(settings, &IID_SETTINGS3)) {
            let _ = put_bool(s3, 24, 0); // SetAreBrowserAcceleratorKeysEnabled
            release(s3);
        }
        if let Some(s5) = non_null(query_interface(settings, &IID_SETTINGS5)) {
            let _ = put_bool(s5, 30, 0); // SetIsPinchZoomEnabled
            release(s5);
        }
        if let Some(s6) = non_null(query_interface(settings, &IID_SETTINGS6)) {
            let _ = put_bool(s6, 32, 0); // SetIsSwipeNavigationEnabled
            release(s6);
        }
        release(settings);
    }
}

fn non_null(p: *mut c_void) -> Option<*mut c_void> {
    if p.is_null() {
        None
    } else {
        Some(p)
    }
}

fn message_loop() {
    unsafe {
        let mut msg = mem::zeroed::<MSG>();
        while GetMessageW(&mut msg, ptr::null_mut(), 0, 0) > 0 {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

fn pump_pending() -> bool {
    unsafe {
        let mut msg = mem::zeroed::<MSG>();
        while PeekMessageW(&mut msg, ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
            if msg.message == windows_sys::Win32::UI::WindowsAndMessaging::WM_QUIT {
                PostQuitMessage(0);
                return false;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    true
}

fn wait_http_pumped(timeout: Duration) -> bool {
    let start = Instant::now();
    loop {
        if fs_viz::shell::http_ok() {
            return true;
        }
        if start.elapsed() >= timeout {
            return false;
        }
        if !pump_pending() {
            return fs_viz::shell::http_ok();
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn prepare_http_then_navigate() {
    fs_viz::shell::start_desktop_host_if_needed();
    let grace = Duration::from_secs(fs_viz::shell::BOOT_GRACE_SECS);
    if wait_http_pumped(grace) {
        return;
    }
    fs_viz::shell::start_desktop_host_if_needed();
    let _ = wait_http_pumped(grace);
}

fn try_navigate() -> bool {
    if !fs_viz::shell::http_ok() {
        return false;
    }
    let url = NAV_URL.with(|u| u.borrow().clone());
    if url.is_empty() {
        return false;
    }
    let url_w = wide(&url);
    CONTROLLER.with(|slot| {
        let controller = slot.get();
        if controller.is_null() {
            return false;
        }
        type GetObj = unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> i32;
        let get_webview: GetObj = unsafe { mem::transmute(*vtbl(controller).add(25)) };
        let mut webview = ptr::null_mut();
        let hr = unsafe { get_webview(controller, &mut webview) };
        if hr < 0 || webview.is_null() {
            return false;
        }
        type Navigate = unsafe extern "system" fn(*mut c_void, *const u16) -> i32;
        let navigate: Navigate = unsafe { mem::transmute(*vtbl(webview).add(5)) };
        let hr = unsafe { navigate(webview, url_w.as_ptr()) };
        unsafe { release(webview) };
        hr >= 0
    })
}

fn on_nav_timer(hwnd: HWND) {
    if NAV_LOADED.get() {
        unsafe {
            let _ = KillTimer(hwnd, NAV_TIMER_ID);
        }
        return;
    }
    if try_navigate() {
        NAV_LOADED.set(true);
        unsafe {
            let _ = KillTimer(hwnd, NAV_TIMER_ID);
        }
        return;
    }
    let next = NAV_BACKOFF_MS.get().saturating_mul(2).min(2000);
    NAV_BACKOFF_MS.set(next);
    unsafe {
        let _ = SetTimer(hwnd, NAV_TIMER_ID, next, None);
    }
}

/// Second `stardust-view --desktop` joins the existing cover instead of creating another host.
fn claim_desktop_instance() -> bool {
    let name = wide("Local\\StarDustViewDesktop");
    let handle = unsafe { CreateMutexW(ptr::null(), 1, name.as_ptr()) };
    if handle.is_null() {
        return true;
    }
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        unsafe {
            let _ = CloseHandle(handle);
        }
        return false;
    }
    true
}

pub fn run(url: &str, user_data: &Path, stardust_desktop: bool) -> io::Result<()> {
    let mode = fs_viz::shell::load_mode().mode;
    let (url, stardust_desktop) = fs_viz::shell::native_view_launch(url, stardust_desktop, mode);
    unsafe {
        // GNU delay-load of SetProcessDpiAwarenessContext currently AVs (BEX64).
        let hr = CoInitializeEx(ptr::null(), COINIT_APARTMENTTHREADED as u32);
        if hr < 0 {
            return Err(hr_err("CoInitializeEx", hr));
        }
    }
    if stardust_desktop && !claim_desktop_instance() {
        fs_viz::shell::restore_stardust_cover();
        return Ok(());
    }
    let hwnd = create_hwnd(stardust_desktop)?;
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = UpdateWindow(hwnd);
    }
    fs_viz::shell::start_desktop_host_if_needed();
    NAV_URL.with(|slot| {
        *slot.borrow_mut() = url.to_string();
    });
    let create_env = load_create_env()?;
    let data = wide(&user_data.to_string_lossy());
    let (env_handler, env_rx) = make_handler(IID_ENV_HANDLER, &ENV_VTBL);
    let hr = unsafe {
        create_env(
            ptr::null(),
            data.as_ptr(),
            ptr::null_mut(),
            env_handler.cast(),
        )
    };
    unsafe { release(env_handler.cast()) };
    if hr < 0 {
        return Err(hr_err("CreateCoreWebView2EnvironmentWithOptions", hr));
    }
    let env = wait_with_pump(env_rx)?;
    let (ctrl_handler, ctrl_rx) = make_handler(IID_CTRL_HANDLER, &CTRL_VTBL);
    type CreateCtrl = unsafe extern "system" fn(*mut c_void, HWND, *mut c_void) -> i32;
    let create_ctrl: CreateCtrl = unsafe { mem::transmute(*vtbl(env).add(3)) };
    let hr = unsafe { create_ctrl(env, hwnd, ctrl_handler.cast()) };
    unsafe { release(ctrl_handler.cast()) };
    if hr < 0 {
        unsafe { release(env) };
        return Err(hr_err("CreateCoreWebView2Controller", hr));
    }
    let controller = match wait_with_pump(ctrl_rx) {
        Ok(p) => p,
        Err(e) => {
            unsafe { release(env) };
            return Err(e);
        }
    };
    unsafe { release(env) };
    apply_bounds(controller, hwnd);
    unsafe {
        let _ = put_bool(controller, 4, 1); // SetIsVisible
    }
    if stardust_desktop {
        fs_viz::shell::apply_cover_non_rude(hwnd);
    }
    type GetObj = unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> i32;
    let get_webview: GetObj = unsafe { mem::transmute(*vtbl(controller).add(25)) };
    let mut webview = ptr::null_mut();
    let hr = unsafe { get_webview(controller, &mut webview) };
    if hr < 0 || webview.is_null() {
        unsafe { release(controller) };
        return Err(hr_err("CoreWebView2", hr));
    }
    apply_settings(webview);
    unsafe { release(webview) };
    type MoveFocus = unsafe extern "system" fn(*mut c_void, i32) -> i32;
    let move_focus: MoveFocus = unsafe { mem::transmute(*vtbl(controller).add(12)) };
    let _ = unsafe { move_focus(controller, MOVE_FOCUS_PROGRAMMATIC) };
    CONTROLLER.with(|slot| {
        unsafe { add_ref(controller) };
        slot.set(controller);
    });
    unsafe { release(controller) };
    prepare_http_then_navigate();
    if try_navigate() {
        NAV_LOADED.set(true);
    } else {
        unsafe {
            let _ = SetTimer(hwnd, NAV_TIMER_ID, 250, None);
        }
    }
    message_loop();
    Ok(())
}
