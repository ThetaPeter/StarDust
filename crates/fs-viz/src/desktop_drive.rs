//! First-party OS-window drive for the StarDust UI agent. Not cosmos `look-at-view`.
//! Never moves the hardware cursor. Never HWND_TOPMOST on StarDustHost.

use std::ffi::c_void;
use std::mem::{size_of, zeroed};
use std::path::{Path, PathBuf};
use std::ptr;
use std::sync::{Mutex, Once, OnceLock};
use std::time::Duration;

use serde_json::{json, Value};
use windows_sys::Win32::Foundation::{BOOL, HWND, LPARAM, POINT, RECT};

use crate::host::{ApiError, ApiResult};

const MAX_RUN_STEPS: usize = 20;
const JPEG_QUALITY: u32 = 72;
const MAX_EDGE: i32 = 1600;
const GRID: i32 = 100;
const TINY_CLIENT_W: i32 = 200;
const TINY_CLIENT_H: i32 = 40;
const CLOAK_ORIGIN: i32 = 32000;
const MAX_HITS: usize = 120;
const CLASS_HOST: &str = "StarDustHost";
const UNDOCK_MARGIN: i32 = 16;
const MIN_SLOT: i32 = 8;

static OLE_ONCE: Once = Once::new();
static PRINT_WINDOW: OnceLock<Option<PrintWindowFn>> = OnceLock::new();
static GDIP_TOKEN: Mutex<Option<usize>> = Mutex::new(None);
static STATE: Mutex<DriveState> = Mutex::new(DriveState {
    target: 0,
    bmp_w: 0,
    bmp_h: 0,
    origin_x: 0,
    origin_y: 0,
    src_w: 0,
    src_h: 0,
    last_title: String::new(),
    last_hits: Vec::new(),
    peek_x: 0,
    peek_y: 0,
    peek_w: 0,
    peek_h: 0,
    peek_held: false,
    peek_active: false,
    peek_hwnd: 0,
    peek_prev_fg: 0,
    undocked: Vec::new(),
    slotted: Vec::new(),
    saved_hwnd: 0,
    saved_l: 0,
    saved_t: 0,
    saved_w: 0,
    saved_h: 0,
    saved_ok: false,
});

struct DriveState {
    target: usize,
    bmp_w: i32,
    bmp_h: i32,
    origin_x: i32,
    origin_y: i32,
    src_w: i32,
    src_h: i32,
    last_title: String,
    last_hits: Vec<Hit>,
    peek_x: i32,
    peek_y: i32,
    peek_w: i32,
    peek_h: i32,
    peek_held: bool,
    peek_active: bool,
    peek_hwnd: usize,
    peek_prev_fg: usize,
    undocked: Vec<usize>,
    slotted: Vec<usize>,
    saved_hwnd: usize,
    saved_l: i32,
    saved_t: i32,
    saved_w: i32,
    saved_h: i32,
    saved_ok: bool,
}

type PrintWindowFn = unsafe extern "system" fn(HWND, windows_sys::Win32::Graphics::Gdi::HDC, u32) -> BOOL;

fn ole_init() {
    OLE_ONCE.call_once(|| unsafe {
        let _ = windows_sys::Win32::System::Ole::OleInitialize(ptr::null_mut());
    });
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn class_of(hwnd: HWND) -> String {
    use windows_sys::Win32::UI::WindowsAndMessaging::GetClassNameW;
    let mut buf = [0u16; 256];
    let n = unsafe { GetClassNameW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
    if n <= 0 {
        return String::new();
    }
    String::from_utf16_lossy(&buf[..n as usize])
}

fn title_of(hwnd: HWND) -> String {
    use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowTextW;
    let mut buf = [0u16; 512];
    let n = unsafe { GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
    if n <= 0 {
        return String::new();
    }
    String::from_utf16_lossy(&buf[..n as usize])
}

/// Visible page leftover after a browser suffix. Never AppData. Empty/browser-only → None.
fn title_browser_url(title: &str) -> Option<String> {
    const SUFFIXES: &[&str] = &[
        " - google chrome",
        " - microsoft edge",
        " - mozilla firefox",
    ];
    const BROWSER_NAMES: &[&str] = &[
        "Google Chrome",
        "Microsoft Edge",
        "Mozilla Firefox",
        "Chrome",
        "Edge",
        "Firefox",
    ];
    let lower = title.to_ascii_lowercase();
    for suf in SUFFIXES {
        if let Some(idx) = lower.rfind(suf) {
            if idx + suf.len() != lower.len() {
                continue;
            }
            let prefix = title[..idx].trim();
            if prefix.is_empty() {
                return None;
            }
            if BROWSER_NAMES.iter().any(|b| prefix.eq_ignore_ascii_case(b)) {
                return None;
            }
            return Some(prefix.to_string());
        }
    }
    None
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
        Path::new(&path)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default()
    }
}

fn hwnd_from_usize(h: usize) -> HWND {
    h as HWND
}

fn usize_from_hwnd(h: HWND) -> usize {
    h as usize
}

fn window_rect(hwnd: HWND) -> Option<RECT> {
    use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowRect;
    let mut r = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    if unsafe { GetWindowRect(hwnd, &mut r) } == 0 {
        None
    } else {
        Some(r)
    }
}

fn hwnd_is_iconic(hwnd: HWND) -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::IsIconic;
    !hwnd.is_null() && unsafe { IsIconic(hwnd) } != 0
}

fn json_i32(args: &Value, key: &str) -> Option<i32> {
    let x = args.get(key)?;
    if let Some(n) = x.as_i64() {
        return Some(n as i32);
    }
    if let Some(n) = x.as_u64() {
        if n > i32::MAX as u64 {
            return None;
        }
        return Some(n as i32);
    }
    if let Some(n) = x.as_f64() {
        if !n.is_finite() {
            return None;
        }
        return Some(n.round() as i32);
    }
    if let Some(s) = x.as_str() {
        let t = s.trim();
        if let Ok(n) = t.parse::<i32>() {
            return Some(n);
        }
        if let Ok(n) = t.parse::<f64>() {
            if n.is_finite() {
                return Some(n.round() as i32);
            }
        }
    }
    None
}

fn stored_peek_slot() -> Option<(i32, i32, i32, i32)> {
    let st = STATE.lock().unwrap_or_else(|e| e.into_inner());
    if st.peek_w < MIN_SLOT || st.peek_h < MIN_SLOT {
        None
    } else {
        Some((st.peek_x, st.peek_y, st.peek_w, st.peek_h))
    }
}

fn set_peek_slot(args: &Value) -> ApiResult {
    let x = json_i32(args, "x").ok_or_else(|| ApiError::bad("peek-slot needs x"))?;
    let y = json_i32(args, "y").ok_or_else(|| ApiError::bad("peek-slot needs y"))?;
    let w = json_i32(args, "w").ok_or_else(|| ApiError::bad("peek-slot needs w"))?;
    let h = json_i32(args, "h").ok_or_else(|| ApiError::bad("peek-slot needs h"))?;
    if w < MIN_SLOT || h < MIN_SLOT {
        return Err(ApiError::bad("peek-slot cosmos rect is too small"));
    }
    let mut st = STATE.lock().unwrap_or_else(|e| e.into_inner());
    st.peek_x = x;
    st.peek_y = y;
    st.peek_w = w;
    st.peek_h = h;
    drop(st);
    Ok(json!({ "ok": true, "x": x, "y": y, "w": w, "h": h }))
}

fn fit_into_slot(win_w: i32, win_h: i32, slot_w: i32, slot_h: i32) -> (i32, i32) {
    (win_w.min(slot_w).max(1), win_h.min(slot_h).max(1))
}

/// Restore the pre-burst foreground unless the user switched to a third app.
/// Never restore StarDustHost.
fn should_restore_prev_fg(now: usize, target: usize, prev: usize, prev_is_host: bool) -> bool {
    if prev == 0 || prev_is_host {
        return false;
    }
    let switched = now != 0 && now != target && now != prev;
    !switched
}

/// Drag = undock: origin left the dock slot by more than 16px in x or y.
/// Resize in place (origin still near the slot) stays docked.
fn drag_away_from_dock(wx: i32, wy: i32, sx: i32, sy: i32) -> bool {
    (wx - sx).abs() > UNDOCK_MARGIN || (wy - sy).abs() > UNDOCK_MARGIN
}

fn hwnd_is_undocked(hwnd: HWND) -> bool {
    let id = usize_from_hwnd(hwnd);
    STATE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .undocked
        .contains(&id)
}

fn hwnd_was_slotted(hwnd: HWND) -> bool {
    let id = usize_from_hwnd(hwnd);
    STATE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .slotted
        .contains(&id)
}

fn mark_hwnd_undocked(hwnd: HWND) {
    if hwnd.is_null() {
        return;
    }
    let id = usize_from_hwnd(hwnd);
    let mut st = STATE.lock().unwrap_or_else(|e| e.into_inner());
    if !st.undocked.contains(&id) {
        st.undocked.push(id);
    }
}

fn mark_hwnd_slotted(hwnd: HWND) {
    if hwnd.is_null() {
        return;
    }
    let id = usize_from_hwnd(hwnd);
    let mut st = STATE.lock().unwrap_or_else(|e| e.into_inner());
    if !st.slotted.contains(&id) {
        st.slotted.push(id);
    }
}

fn note_undock_from_rect(hwnd: HWND) {
    if hwnd.is_null() || hwnd_is_iconic(hwnd) || hwnd_is_undocked(hwnd) {
        return;
    }
    if !hwnd_was_slotted(hwnd) {
        return;
    }
    let Some(r) = window_rect(hwnd) else {
        return;
    };
    if r.left.abs() >= CLOAK_ORIGIN || r.top.abs() >= CLOAK_ORIGIN {
        return;
    }
    let Some((sx, sy, _, _)) = stored_peek_slot() else {
        return;
    };
    if drag_away_from_dock(r.left, r.top, sx, sy) {
        mark_hwnd_undocked(hwnd);
    }
}

fn restore_driven_visible(hwnd: HWND) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{IsWindowVisible, ShowWindow, SW_RESTORE};
    if hwnd.is_null() {
        return;
    }
    unsafe {
        if hwnd_is_iconic(hwnd) || IsWindowVisible(hwnd) == 0 {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
    }
}

fn capture_user_frame(hwnd: HWND) {
    if hwnd.is_null() || hwnd_is_iconic(hwnd) {
        return;
    }
    let id = usize_from_hwnd(hwnd);
    let Some(r) = window_rect(hwnd) else {
        return;
    };
    let w = (r.right - r.left).max(0);
    let h = (r.bottom - r.top).max(0);
    if w < MIN_SLOT || h < MIN_SLOT {
        return;
    }
    if r.left.abs() >= CLOAK_ORIGIN || r.top.abs() >= CLOAK_ORIGIN {
        return;
    }
    let in_slot = stored_peek_slot()
        .map(|(sx, sy, _, _)| !drag_away_from_dock(r.left, r.top, sx, sy))
        .unwrap_or(false);
    {
        let st = STATE.lock().unwrap_or_else(|e| e.into_inner());
        if st.saved_ok && st.saved_hwnd == id && (in_slot || hwnd_is_iconic(hwnd)) {
            return;
        }
        if st.saved_ok && st.saved_hwnd == id {
            return;
        }
    }
    if in_slot {
        return;
    }
    let mut st = STATE.lock().unwrap_or_else(|e| e.into_inner());
    st.saved_hwnd = id;
    st.saved_l = r.left;
    st.saved_t = r.top;
    st.saved_w = w;
    st.saved_h = h;
    st.saved_ok = true;
}

/// Place the driven HWND in the cosmos slot. Never SetWindowPos StarDustHost.
/// HWND_TOP on the driven window only (not TOPMOST, not on the host).
fn place_in_slot(hwnd: HWND) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SetWindowPos, HWND_TOP, SWP_NOACTIVATE, SWP_NOOWNERZORDER, SWP_SHOWWINDOW,
    };
    if hwnd.is_null() || class_of(hwnd) == CLASS_HOST {
        return;
    }
    restore_driven_visible(hwnd);
    note_undock_from_rect(hwnd);
    if hwnd_is_undocked(hwnd) {
        return;
    }
    capture_user_frame(hwnd);
    let Some((sx, sy, sw, sh)) = stored_peek_slot() else {
        return;
    };
    let (nw, nh) = match window_rect(hwnd) {
        Some(r) => {
            let cw = (r.right - r.left).max(1);
            let ch = (r.bottom - r.top).max(1);
            fit_into_slot(cw, ch, sw, sh)
        }
        None => (sw.max(1), sh.max(1)),
    };
    unsafe {
        let _ = SetWindowPos(
            hwnd,
            HWND_TOP,
            sx,
            sy,
            nw,
            nh,
            SWP_SHOWWINDOW | SWP_NOACTIVATE | SWP_NOOWNERZORDER,
        );
    }
    mark_hwnd_slotted(hwnd);
}

fn raise_target(target: HWND) -> HWND {
    use windows_sys::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowThreadProcessId, SetForegroundWindow,
    };
    let prev_fg = unsafe { GetForegroundWindow() };
    if target.is_null() || class_of(target) == CLASS_HOST {
        return prev_fg;
    }
    unsafe {
        let our = GetCurrentThreadId();
        let mut pid = 0u32;
        let their = GetWindowThreadProcessId(target, &mut pid);
        if their != 0 && their != our {
            let _ = AttachThreadInput(our, their, 1);
        }
        let _ = SetForegroundWindow(target);
        if their != 0 && their != our {
            let _ = AttachThreadInput(our, their, 0);
        }
    }
    prev_fg
}

fn peek_enter(hwnd: HWND) {
    if hwnd.is_null() || class_of(hwnd) == CLASS_HOST {
        return;
    }
    note_undock_from_rect(hwnd);
    let (active, same) = {
        let st = STATE.lock().unwrap_or_else(|e| e.into_inner());
        let id = usize_from_hwnd(hwnd);
        (st.peek_active, st.peek_active && st.peek_hwnd == id)
    };
    if same {
        place_in_slot(hwnd);
        return;
    }
    if active {
        peek_finish();
    }
    place_in_slot(hwnd);
    let prev_fg = raise_target(hwnd);
    let mut st = STATE.lock().unwrap_or_else(|e| e.into_inner());
    st.peek_active = true;
    st.peek_hwnd = usize_from_hwnd(hwnd);
    st.peek_prev_fg = usize_from_hwnd(prev_fg);
}

fn peek_finish() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, SetForegroundWindow};
    let (target, prev) = {
        let mut st = STATE.lock().unwrap_or_else(|e| e.into_inner());
        if !st.peek_active {
            return;
        }
        let target = hwnd_from_usize(st.peek_hwnd);
        let prev = hwnd_from_usize(st.peek_prev_fg);
        st.peek_active = false;
        st.peek_hwnd = 0;
        st.peek_prev_fg = 0;
        (target, prev)
    };
    if target.is_null() || class_of(target) == CLASS_HOST {
        return;
    }
    note_undock_from_rect(target);
    let now = unsafe { GetForegroundWindow() };
    let prev_is_host = !prev.is_null() && class_of(prev) == CLASS_HOST;
    if should_restore_prev_fg(
        usize_from_hwnd(now),
        usize_from_hwnd(target),
        usize_from_hwnd(prev),
        prev_is_host,
    ) && !prev.is_null()
        && !prev_is_host
    {
        unsafe {
            let _ = SetForegroundWindow(prev);
        }
    }
}

fn peek_leave_unless_held() {
    let held = STATE.lock().unwrap_or_else(|e| e.into_inner()).peek_held;
    if !held {
        peek_finish();
    }
}

fn with_peek<T>(hwnd: HWND, f: impl FnOnce() -> T) -> T {
    peek_enter(hwnd);
    let out = f();
    peek_leave_unless_held();
    out
}

struct RunPeekHold;

impl RunPeekHold {
    fn enter() -> Self {
        STATE.lock().unwrap_or_else(|e| e.into_inner()).peek_held = true;
        Self
    }
}

impl Drop for RunPeekHold {
    fn drop(&mut self) {
        peek_finish();
        STATE.lock().unwrap_or_else(|e| e.into_inner()).peek_held = false;
    }
}

pub(crate) fn forbidden_path(p: &Path) -> bool {
    p.components().any(|c| {
        let n = c.as_os_str().to_string_lossy();
        n.eq_ignore_ascii_case("AppData")
            || n.eq_ignore_ascii_case("Application Data")
            || n.to_ascii_lowercase().starts_with("stardust-ws-")
    })
}

fn reject_drive_path(p: &Path) -> Result<(), ApiError> {
    if forbidden_path(p) {
        return Err(ApiError::bad(
            "path is under AppData, Application Data, or stardust-ws-*",
        ));
    }
    Ok(())
}

fn parse_abs_file(s: &str) -> Result<PathBuf, ApiError> {
    let p = crate::host::parse_abs(s)?;
    reject_drive_path(&p)?;
    if !p.is_file() {
        return Err(ApiError::bad("drop-file source must be an existing file"));
    }
    Ok(p)
}

fn is_shell_chrome(class: &str) -> bool {
    class.eq_ignore_ascii_case("Progman")
        || class.eq_ignore_ascii_case("WorkerW")
        || class.eq_ignore_ascii_case("Shell_TrayWnd")
        || class.eq_ignore_ascii_case("Shell_SecondaryTrayWnd")
}

fn print_window() -> Option<PrintWindowFn> {
    *PRINT_WINDOW.get_or_init(|| unsafe {
        use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
        let user32 = GetModuleHandleW(wide("user32").as_ptr());
        if user32.is_null() {
            return None;
        }
        let p = GetProcAddress(user32, b"PrintWindow\0".as_ptr());
        p.map(|f| std::mem::transmute::<_, PrintWindowFn>(f))
    })
}

fn gdip_start() -> Result<(), ApiError> {
    use windows_sys::Win32::Graphics::GdiPlus::{GdiplusStartup, GdiplusStartupInput};
    let mut slot = GDIP_TOKEN.lock().unwrap_or_else(|e| e.into_inner());
    if slot.is_some() {
        return Ok(());
    }
    let input = GdiplusStartupInput {
        GdiplusVersion: 1,
        DebugEventCallback: 0,
        SuppressBackgroundThread: 0,
        SuppressExternalCodecs: 0,
    };
    let mut token = 0usize;
    let st = unsafe { GdiplusStartup(&mut token, &input, ptr::null_mut()) };
    if st != 0 {
        return Err(ApiError::internal("GdiplusStartup failed"));
    }
    *slot = Some(token);
    Ok(())
}

fn jpeg_clsid() -> Result<windows_sys::core::GUID, ApiError> {
    use windows_sys::Win32::Graphics::GdiPlus::{GdipGetImageEncoders, GdipGetImageEncodersSize, ImageCodecInfo};
    let mut num = 0u32;
    let mut bytes = 0u32;
    if unsafe { GdipGetImageEncodersSize(&mut num, &mut bytes) } != 0 || bytes == 0 {
        return Err(ApiError::internal("no image encoders"));
    }
    let mut buf = vec![0u8; bytes as usize];
    if unsafe { GdipGetImageEncoders(num, bytes, buf.as_mut_ptr() as *mut ImageCodecInfo) } != 0 {
        return Err(ApiError::internal("GdipGetImageEncoders failed"));
    }
    let info = unsafe {
        std::slice::from_raw_parts(buf.as_ptr() as *const ImageCodecInfo, num as usize)
    };
    for enc in info {
        if enc.MimeType.is_null() {
            continue;
        }
        let mime = unsafe {
            let mut n = 0usize;
            while *enc.MimeType.add(n) != 0 {
                n += 1;
            }
            String::from_utf16_lossy(std::slice::from_raw_parts(enc.MimeType, n))
        };
        if mime.eq_ignore_ascii_case("image/jpeg") {
            return Ok(enc.Clsid);
        }
    }
    Err(ApiError::internal("JPEG encoder missing"))
}

fn encode_hbitmap(hbmp: windows_sys::Win32::Graphics::Gdi::HBITMAP) -> Result<Vec<u8>, ApiError> {
    use windows_sys::Win32::Graphics::GdiPlus::{
        GdipCreateBitmapFromHBITMAP, GdipDisposeImage, GdipSaveImageToStream, EncoderParameter,
        EncoderParameterValueTypeLong, EncoderParameters, EncoderQuality, GpBitmap, GpImage,
    };
    use windows_sys::Win32::System::Com::StructuredStorage::{
        CreateStreamOnHGlobal, GetHGlobalFromStream,
    };
    use windows_sys::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};
    gdip_start()?;
    let clsid = jpeg_clsid()?;
    let mut gp: *mut GpBitmap = ptr::null_mut();
    if unsafe { GdipCreateBitmapFromHBITMAP(hbmp, ptr::null_mut(), &mut gp) } != 0 || gp.is_null() {
        return Err(ApiError::internal("GdipCreateBitmapFromHBITMAP failed"));
    }
    let mut stream: *mut c_void = ptr::null_mut();
    let hr = unsafe { CreateStreamOnHGlobal(ptr::null_mut(), 1, &mut stream) };
    if hr < 0 || stream.is_null() {
        unsafe {
            let _ = GdipDisposeImage(gp as *mut GpImage);
        }
        return Err(ApiError::internal("CreateStreamOnHGlobal failed"));
    }
    let mut quality = JPEG_QUALITY;
    let params = EncoderParameters {
        Count: 1,
        Parameter: [EncoderParameter {
            Guid: EncoderQuality,
            NumberOfValues: 1,
            Type: EncoderParameterValueTypeLong as u32,
            Value: &mut quality as *mut u32 as *mut c_void,
        }],
    };
    let st = unsafe { GdipSaveImageToStream(gp as *mut GpImage, stream, &clsid, &params) };
    unsafe {
        let _ = GdipDisposeImage(gp as *mut GpImage);
    }
    if st != 0 {
        release_unk(stream);
        return Err(ApiError::internal("GdipSaveImageToStream failed"));
    }
    let mut hg = ptr::null_mut();
    let hr = unsafe { GetHGlobalFromStream(stream, &mut hg) };
    if hr < 0 || hg.is_null() {
        release_unk(stream);
        return Err(ApiError::internal("GetHGlobalFromStream failed"));
    }
    let size = unsafe { GlobalSize(hg) };
    let ptr_mem = unsafe { GlobalLock(hg) };
    if ptr_mem.is_null() || size == 0 {
        unsafe {
            let _ = GlobalUnlock(hg);
        }
        release_unk(stream);
        return Err(ApiError::internal("JPEG stream empty"));
    }
    let bytes = unsafe { std::slice::from_raw_parts(ptr_mem as *const u8, size) }.to_vec();
    unsafe {
        let _ = GlobalUnlock(hg);
    }
    release_unk(stream);
    Ok(bytes)
}

fn release_unk(p: *mut c_void) {
    if p.is_null() {
        return;
    }
    unsafe {
        let vtbl = *(p as *mut *mut usize);
        let rel: unsafe extern "system" fn(*mut c_void) -> u32 = std::mem::transmute(*vtbl.add(2));
        let _ = rel(p);
    }
}

fn b64(data: &[u8]) -> String {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((data.len() + 2) / 3 * 4);
    let mut i = 0;
    while i < data.len() {
        let b0 = data[i] as u32;
        let b1 = if i + 1 < data.len() { data[i + 1] as u32 } else { 0 };
        let b2 = if i + 2 < data.len() { data[i + 2] as u32 } else { 0 };
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(T[((n >> 18) & 63) as usize] as char);
        out.push(T[((n >> 12) & 63) as usize] as char);
        if i + 1 < data.len() {
            out.push(T[((n >> 6) & 63) as usize] as char);
        } else {
            out.push('=');
        }
        if i + 2 < data.len() {
            out.push(T[(n & 63) as usize] as char);
        } else {
            out.push('=');
        }
        i += 3;
    }
    out
}

fn draw_grid(hdc: windows_sys::Win32::Graphics::Gdi::HDC, w: i32, h: i32) {
    use windows_sys::Win32::Graphics::Gdi::{
        CreateFontW, CreatePen, DeleteObject, LineTo, MoveToEx, SelectObject, SetBkMode,
        SetTextColor, TextOutW, CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, DEFAULT_PITCH,
        DEFAULT_QUALITY, FW_BOLD, OUT_DEFAULT_PRECIS, PS_SOLID, TRANSPARENT,
    };
    unsafe {
        let pen = CreatePen(PS_SOLID, 1, 0x00B4B4B4);
        let old = SelectObject(hdc, pen);
        let mut x = 0;
        while x <= w {
            let _ = MoveToEx(hdc, x, 0, ptr::null_mut());
            let _ = LineTo(hdc, x, h);
            x += GRID;
        }
        let mut y = 0;
        while y <= h {
            let _ = MoveToEx(hdc, 0, y, ptr::null_mut());
            let _ = LineTo(hdc, w, y);
            y += GRID;
        }
        let _ = SelectObject(hdc, old);
        let _ = DeleteObject(pen);
        let face = wide("Segoe UI");
        let font = CreateFontW(
            -12,
            0,
            0,
            0,
            FW_BOLD as i32,
            0,
            0,
            0,
            DEFAULT_CHARSET as u32,
            OUT_DEFAULT_PRECIS as u32,
            CLIP_DEFAULT_PRECIS as u32,
            DEFAULT_QUALITY as u32,
            DEFAULT_PITCH as u32,
            face.as_ptr(),
        );
        let old_font = SelectObject(hdc, font);
        SetBkMode(hdc, TRANSPARENT as i32);
        SetTextColor(hdc, 0x0000FFFF);
        let mut n = 1i32;
        let mut cy = 0;
        while cy < h {
            let mut cx = 0;
            while cx < w {
                let label = wide(&n.to_string());
                let _ = TextOutW(hdc, cx + 2, cy + 2, label.as_ptr(), (label.len() - 1) as i32);
                n += 1;
                cx += GRID;
            }
            cy += GRID;
        }
        SetTextColor(hdc, 0x00E0E0E0);
        let size = wide(&format!("{w}x{h} {GRID}px"));
        let _ = TextOutW(
            hdc,
            4,
            h.saturating_sub(18).max(4),
            size.as_ptr(),
            (size.len() - 1) as i32,
        );
        let _ = SelectObject(hdc, old_font);
        if !font.is_null() {
            let _ = DeleteObject(font);
        }
    }
}

fn grid_cells(w: i32, h: i32) -> Vec<Value> {
    let mut cells = Vec::new();
    if w <= 0 || h <= 0 {
        return cells;
    }
    let mut n = 1i32;
    let mut y = 0;
    while y < h {
        let mut x = 0;
        while x < w {
            cells.push(json!({
                "n": n,
                "x": x,
                "y": y,
                "w": GRID.min(w - x),
                "h": GRID.min(h - y),
            }));
            n += 1;
            x += GRID;
        }
        y += GRID;
    }
    cells
}

fn capture_rect(sx: i32, sy: i32, w: i32, h: i32, hwnd: Option<HWND>) -> Result<(Vec<u8>, i32, i32), ApiError> {
    use windows_sys::Win32::Graphics::Gdi::{
        BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC,
        ReleaseDC, SelectObject, SetStretchBltMode, StretchBlt, HALFTONE, SRCCOPY,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::PW_RENDERFULLCONTENT;
    if w <= 0 || h <= 0 {
        return Err(ApiError::bad("empty capture rectangle"));
    }
    unsafe {
        let hdc_screen = GetDC(ptr::null_mut());
        if hdc_screen.is_null() {
            return Err(ApiError::internal("GetDC failed"));
        }
        let hdc_mem = CreateCompatibleDC(hdc_screen);
        let hbmp = CreateCompatibleBitmap(hdc_screen, w, h);
        let old = SelectObject(hdc_mem, hbmp);
        let mut ok = 0;
        if let Some(h) = hwnd {
            if let Some(pw) = print_window() {
                ok = pw(h, hdc_mem, PW_RENDERFULLCONTENT);
            }
        }
        if ok == 0 {
            let _ = BitBlt(hdc_mem, 0, 0, w, h, hdc_screen, sx, sy, SRCCOPY);
        }
        let (ow, oh) = if w.max(h) > MAX_EDGE {
            let scale = MAX_EDGE as f32 / w.max(h) as f32;
            let nw = ((w as f32) * scale).round().max(1.0) as i32;
            let nh = ((h as f32) * scale).round().max(1.0) as i32;
            let hdc2 = CreateCompatibleDC(hdc_screen);
            let hbmp2 = CreateCompatibleBitmap(hdc_screen, nw, nh);
            let old2 = SelectObject(hdc2, hbmp2);
            SetStretchBltMode(hdc2, HALFTONE as i32);
            let _ = StretchBlt(hdc2, 0, 0, nw, nh, hdc_mem, 0, 0, w, h, SRCCOPY);
            draw_grid(hdc2, nw, nh);
            let _ = SelectObject(hdc2, old2);
            let jpeg = encode_hbitmap(hbmp2);
            let _ = DeleteObject(hbmp2);
            let _ = DeleteDC(hdc2);
            let _ = SelectObject(hdc_mem, old);
            let _ = DeleteObject(hbmp);
            let _ = DeleteDC(hdc_mem);
            let _ = ReleaseDC(ptr::null_mut(), hdc_screen);
            return Ok((jpeg?, nw, nh));
        } else {
            draw_grid(hdc_mem, w, h);
            (w, h)
        };
        let _ = SelectObject(hdc_mem, old);
        let jpeg = encode_hbitmap(hbmp);
        let _ = DeleteObject(hbmp);
        let _ = DeleteDC(hdc_mem);
        let _ = ReleaseDC(ptr::null_mut(), hdc_screen);
        Ok((jpeg?, ow, oh))
    }
}

#[derive(Clone, Debug)]
struct Hit {
    name: String,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
}

fn hit_json(h: &Hit) -> Value {
    json!({
        "name": h.name,
        "x": h.x,
        "y": h.y,
        "w": h.w,
        "h": h.h,
    })
}

fn screen_box_to_hit(name: String, left: i32, top: i32, right: i32, bottom: i32, origin: RECT, bmp_w: i32, bmp_h: i32) -> Option<Hit> {
    let src_w = (origin.right - origin.left).max(1) as f32;
    let src_h = (origin.bottom - origin.top).max(1) as f32;
    let scale_x = bmp_w as f32 / src_w;
    let scale_y = bmp_h as f32 / src_h;
    let x = ((left - origin.left) as f32 * scale_x).round() as i32;
    let y = ((top - origin.top) as f32 * scale_y).round() as i32;
    let w = ((right - left) as f32 * scale_x).round() as i32;
    let h = ((bottom - top) as f32 * scale_y).round() as i32;
    if name.trim().is_empty() || w < 8 || h < 8 {
        return None;
    }
    let mut name = name.trim().to_string();
    if name.len() > 120 {
        name.truncate(120);
    }
    Some(Hit { name, x, y, w, h })
}

fn push_hit(hits: &mut Vec<Hit>, hit: Hit) {
    if hits.len() >= MAX_HITS {
        return;
    }
    if hits.iter().any(|e| {
        e.name.eq_ignore_ascii_case(&hit.name)
            && (e.x - hit.x).abs() <= 8
            && (e.y - hit.y).abs() <= 8
    }) {
        return;
    }
    hits.push(hit);
}

fn point_in_hits(x: i32, y: i32, hits: &[Hit]) -> bool {
    hits.iter()
        .any(|h| x >= h.x && y >= h.y && x < h.x + h.w && y < h.y + h.h)
}

fn coord_click_miss(on_hit_box: bool, ui_changed: bool) -> bool {
    !on_hit_box && !ui_changed
}

fn map_bitmap_to_screen(
    x: i32,
    y: i32,
    origin_x: i32,
    origin_y: i32,
    bmp_w: i32,
    bmp_h: i32,
    src_w: i32,
    src_h: i32,
) -> (i32, i32) {
    if bmp_w <= 0 || bmp_h <= 0 || src_w <= 0 || src_h <= 0 {
        return (origin_x + x, origin_y + y);
    }
    (
        origin_x + (x as i64 * src_w as i64 / bmp_w as i64) as i32,
        origin_y + (y as i64 * src_h as i64 / bmp_h as i64) as i32,
    )
}

fn window_unusable(x: i32, y: i32, client_w: i32, client_h: i32, cloaked: bool) -> bool {
    cloaked
        || x.abs() >= CLOAK_ORIGIN
        || y.abs() >= CLOAK_ORIGIN
        || (client_w <= TINY_CLIENT_W && client_h <= TINY_CLIENT_H)
}

fn client_size(hwnd: HWND) -> (i32, i32) {
    use windows_sys::Win32::UI::WindowsAndMessaging::GetClientRect;
    let mut r = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    if unsafe { GetClientRect(hwnd, &mut r) } == 0 {
        (0, 0)
    } else {
        (r.right - r.left, r.bottom - r.top)
    }
}

fn window_cloaked(hwnd: HWND) -> bool {
    use windows_sys::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
    let mut cloaked: u32 = 0;
    let hr = unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_CLOAKED as u32,
            &mut cloaked as *mut u32 as *mut c_void,
            size_of::<u32>() as u32,
        )
    };
    hr >= 0 && cloaked != 0
}

fn json_u64(args: &Value, keys: &[&str]) -> Option<u64> {
    for k in keys {
        let Some(x) = args.get(*k) else { continue };
        if let Some(n) = x.as_u64() {
            return Some(n);
        }
        if let Some(n) = x.as_i64() {
            if n >= 0 {
                return Some(n as u64);
            }
        }
        if let Some(s) = x.as_str() {
            let t = s.trim();
            if t.is_empty() {
                continue;
            }
            if let Ok(n) = t.parse::<u64>() {
                return Some(n);
            }
            if let Some(hex) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
                if let Ok(n) = u64::from_str_radix(hex, 16) {
                    return Some(n);
                }
            }
        }
    }
    None
}

fn json_force(v: &Value) -> bool {
    match v.get("force") {
        Some(x) if x.as_bool() == Some(true) => true,
        Some(x) if x.as_str().is_some_and(|s| s.eq_ignore_ascii_case("true")) => true,
        _ => false,
    }
}

fn expect_contact(step: &Value, run_args: &Value) -> Option<String> {
    for src in [step, run_args] {
        for key in ["contact", "header", "expect"] {
            if let Some(s) = src.get(key).and_then(|x| x.as_str()) {
                let t = s.trim();
                if !t.is_empty() {
                    return Some(t.to_string());
                }
            }
        }
    }
    None
}

fn composer_allows(expect: &str, force: bool, last_title: &str, hit_names: &[String]) -> bool {
    if force {
        return true;
    }
    let n = expect.trim().to_ascii_lowercase();
    if n.is_empty() {
        return true;
    }
    if last_title.to_ascii_lowercase().contains(&n) {
        return true;
    }
    hit_names
        .iter()
        .any(|h| h.to_ascii_lowercase().contains(&n))
}

fn is_type_or_enter(step: &Value) -> bool {
    let op = step.get("op").and_then(|v| v.as_str()).unwrap_or("");
    if op != "window-type" {
        return false;
    }
    let text = step.get("text").and_then(|v| v.as_str()).unwrap_or("").trim();
    if !text.is_empty() {
        return true;
    }
    let chord = step
        .get("chord")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    chord == "enter" || chord == "return" || chord == "ctrl+enter"
}

fn fnv64(data: &[u8]) -> u64 {
    let mut h = 0xcbf29ce484222325u64;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

struct HitWalk {
    bmp_w: i32,
    bmp_h: i32,
    src: RECT,
    hits: Vec<Hit>,
}

unsafe extern "system" fn enum_child_hits(hwnd: HWND, lparam: LPARAM) -> BOOL {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetWindowRect, IsWindowVisible};
    let walk = lparam as *mut HitWalk;
    if walk.is_null() {
        return 0;
    }
    unsafe {
        if IsWindowVisible(hwnd) == 0 {
            return 1;
        }
        let name = title_of(hwnd);
        if name.is_empty() {
            return 1;
        }
        let mut r = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        if GetWindowRect(hwnd, &mut r) == 0 {
            return 1;
        }
        if let Some(hit) = screen_box_to_hit(
            name,
            r.left,
            r.top,
            r.right,
            r.bottom,
            (*walk).src,
            (*walk).bmp_w,
            (*walk).bmp_h,
        ) {
            push_hit(&mut (*walk).hits, hit);
        }
    }
    1
}

fn child_hits_raw(hwnd: HWND, origin: RECT, bmp_w: i32, bmp_h: i32) -> Vec<Hit> {
    use windows_sys::Win32::UI::WindowsAndMessaging::EnumChildWindows;
    let mut walk = HitWalk {
        bmp_w,
        bmp_h,
        src: origin,
        hits: Vec::new(),
    };
    unsafe {
        let _ = EnumChildWindows(hwnd, Some(enum_child_hits), &mut walk as *mut _ as LPARAM);
    }
    walk.hits
}

unsafe fn variant_vt(var: &windows_sys::Win32::System::Variant::VARIANT) -> u16 {
    var.Anonymous.Anonymous.vt
}

unsafe fn variant_bstr(var: &windows_sys::Win32::System::Variant::VARIANT) -> String {
    use windows_sys::Win32::Foundation::SysStringLen;
    use windows_sys::Win32::System::Variant::VT_BSTR;
    if variant_vt(var) != VT_BSTR {
        return String::new();
    }
    let b = var.Anonymous.Anonymous.Anonymous.bstrVal;
    if b.is_null() {
        return String::new();
    }
    let n = SysStringLen(b);
    if n == 0 {
        String::new()
    } else {
        String::from_utf16_lossy(std::slice::from_raw_parts(b, n as usize))
    }
}

unsafe fn variant_bool_true(var: &windows_sys::Win32::System::Variant::VARIANT) -> bool {
    use windows_sys::Win32::System::Variant::VT_BOOL;
    if variant_vt(var) != VT_BOOL {
        return false;
    }
    var.Anonymous.Anonymous.Anonymous.boolVal != 0
}

unsafe fn variant_rect4(var: &windows_sys::Win32::System::Variant::VARIANT) -> Option<(f64, f64, f64, f64)> {
    use windows_sys::Win32::System::Ole::{SafeArrayAccessData, SafeArrayGetUBound, SafeArrayUnaccessData};
    use windows_sys::Win32::System::Variant::{VT_ARRAY, VT_R8};
    let vt = variant_vt(var);
    if vt & VT_ARRAY == 0 || (vt & 0xFFF) != VT_R8 {
        return None;
    }
    let psa = var.Anonymous.Anonymous.Anonymous.parray;
    if psa.is_null() {
        return None;
    }
    let mut hi = 0i32;
    if SafeArrayGetUBound(psa, 1, &mut hi) < 0 || hi < 3 {
        return None;
    }
    let mut data: *mut c_void = ptr::null_mut();
    if SafeArrayAccessData(psa, &mut data) < 0 || data.is_null() {
        return None;
    }
    let p = data as *const f64;
    let out = (*p, *p.add(1), *p.add(2), *p.add(3));
    let _ = SafeArrayUnaccessData(psa);
    Some(out)
}

unsafe fn uia_prop(
    node: windows_sys::Win32::UI::Accessibility::HUIANODE,
    id: i32,
) -> windows_sys::Win32::System::Variant::VARIANT {
    use windows_sys::Win32::UI::Accessibility::UiaGetPropertyValue;
    let mut var = zeroed();
    let _ = UiaGetPropertyValue(node, id, &mut var);
    var
}

unsafe fn uia_node_hit(
    node: windows_sys::Win32::UI::Accessibility::HUIANODE,
    origin: RECT,
    bmp_w: i32,
    bmp_h: i32,
) -> Option<Hit> {
    use windows_sys::Win32::System::Variant::VariantClear;
    use windows_sys::Win32::UI::Accessibility::{
        UIA_BoundingRectanglePropertyId, UIA_IsOffscreenPropertyId, UIA_NamePropertyId,
        UIA_ValueValuePropertyId,
    };
    let mut name_v = uia_prop(node, UIA_NamePropertyId);
    let mut name = variant_bstr(&name_v);
    let _ = VariantClear(&mut name_v);
    if name.is_empty() {
        let mut val_v = uia_prop(node, UIA_ValueValuePropertyId);
        name = variant_bstr(&val_v);
        let _ = VariantClear(&mut val_v);
    }
    let mut off_v = uia_prop(node, UIA_IsOffscreenPropertyId);
    let off = variant_bool_true(&off_v);
    let _ = VariantClear(&mut off_v);
    if off {
        return None;
    }
    let mut rect_v = uia_prop(node, UIA_BoundingRectanglePropertyId);
    let rect = variant_rect4(&rect_v);
    let _ = VariantClear(&mut rect_v);
    let (left, top, width, height) = rect?;
    if !width.is_finite() || !height.is_finite() {
        return None;
    }
    screen_box_to_hit(
        name,
        left.round() as i32,
        top.round() as i32,
        (left + width).round() as i32,
        (top + height).round() as i32,
        origin,
        bmp_w,
        bmp_h,
    )
}

unsafe fn uia_neighbor(
    node: windows_sys::Win32::UI::Accessibility::HUIANODE,
    dir: windows_sys::Win32::UI::Accessibility::NavigateDirection,
) -> windows_sys::Win32::UI::Accessibility::HUIANODE {
    use windows_sys::Win32::Foundation::SysFreeString;
    use windows_sys::Win32::System::Ole::SafeArrayDestroy;
    use windows_sys::Win32::System::Variant::VariantClear;
    use windows_sys::Win32::UI::Accessibility::{
        UiaHUiaNodeFromVariant, UiaNavigate, AutomationElementMode_Full, ConditionType_True,
        TreeScope_Element, UiaCacheRequest, UiaCondition,
    };
    let mut cond = UiaCondition {
        ConditionType: ConditionType_True,
    };
    let mut req = UiaCacheRequest {
        pViewCondition: &mut cond,
        Scope: TreeScope_Element,
        pProperties: ptr::null_mut(),
        cProperties: 0,
        pPatterns: ptr::null_mut(),
        cPatterns: 0,
        automationElementMode: AutomationElementMode_Full,
    };
    let mut data = ptr::null_mut();
    let mut tree: windows_sys::core::BSTR = ptr::null();
    let hr = UiaNavigate(node, dir, &mut cond, &mut req, &mut data, &mut tree);
    if !tree.is_null() {
        SysFreeString(tree);
    }
    if hr < 0 || data.is_null() {
        return ptr::null_mut();
    }
    let mut lo = 0i32;
    let _ = windows_sys::Win32::System::Ole::SafeArrayGetLBound(data, 1, &mut lo);
    let mut var = zeroed();
    let got = windows_sys::Win32::System::Ole::SafeArrayGetElement(
        data,
        &lo,
        &mut var as *mut _ as *mut c_void,
    );
    let mut out = ptr::null_mut();
    if got >= 0 {
        let _ = UiaHUiaNodeFromVariant(&mut var, &mut out);
    }
    let _ = VariantClear(&mut var);
    let _ = SafeArrayDestroy(data);
    out
}

unsafe fn walk_uia(
    node: windows_sys::Win32::UI::Accessibility::HUIANODE,
    origin: RECT,
    bmp_w: i32,
    bmp_h: i32,
    hits: &mut Vec<Hit>,
    depth: u32,
) {
    use windows_sys::Win32::UI::Accessibility::{
        UiaNodeRelease, NavigateDirection_FirstChild, NavigateDirection_NextSibling,
    };
    if hits.len() >= MAX_HITS || depth > 12 || node.is_null() {
        return;
    }
    if let Some(hit) = uia_node_hit(node, origin, bmp_w, bmp_h) {
        push_hit(hits, hit);
    }
    let mut child = uia_neighbor(node, NavigateDirection_FirstChild);
    while !child.is_null() {
        walk_uia(child, origin, bmp_w, bmp_h, hits, depth + 1);
        let next = uia_neighbor(child, NavigateDirection_NextSibling);
        UiaNodeRelease(child);
        child = next;
        if hits.len() >= MAX_HITS {
            if !child.is_null() {
                UiaNodeRelease(child);
            }
            break;
        }
    }
}

unsafe fn uia_find_batch(
    root: windows_sys::Win32::UI::Accessibility::HUIANODE,
    origin: RECT,
    bmp_w: i32,
    bmp_h: i32,
    hits: &mut Vec<Hit>,
) {
    use windows_sys::Win32::System::Ole::{
        SafeArrayDestroy, SafeArrayGetElement, SafeArrayGetLBound, SafeArrayGetUBound,
    };
    use windows_sys::Win32::System::Variant::VariantClear;
    use windows_sys::Win32::UI::Accessibility::{
        UiaFind, AutomationElementMode_None, ConditionType_True, TreeScope_Subtree,
        UIA_BoundingRectanglePropertyId, UIA_IsOffscreenPropertyId, UIA_NamePropertyId,
        UIA_ValueValuePropertyId, UiaCacheRequest, UiaCondition, UiaFindParams,
    };
    let mut cond = UiaCondition {
        ConditionType: ConditionType_True,
    };
    let mut props = [
        UIA_NamePropertyId,
        UIA_ValueValuePropertyId,
        UIA_BoundingRectanglePropertyId,
        UIA_IsOffscreenPropertyId,
    ];
    let mut req = UiaCacheRequest {
        pViewCondition: &mut cond,
        Scope: TreeScope_Subtree,
        pProperties: props.as_mut_ptr(),
        cProperties: props.len() as i32,
        pPatterns: ptr::null_mut(),
        cPatterns: 0,
        automationElementMode: AutomationElementMode_None,
    };
    let mut params = UiaFindParams {
        MaxDepth: 10,
        FindFirst: 0,
        ExcludeRoot: 0,
        pFindCondition: &mut cond,
    };
    let mut data = ptr::null_mut();
    let mut offsets = ptr::null_mut();
    let mut trees = ptr::null_mut();
    let hr = UiaFind(root, &mut params, &mut req, &mut data, &mut offsets, &mut trees);
    if !trees.is_null() {
        let _ = SafeArrayDestroy(trees);
    }
    if !offsets.is_null() {
        let _ = SafeArrayDestroy(offsets);
    }
    if hr < 0 || data.is_null() {
        return;
    }
    let mut lo = 0i32;
    let mut hi = -1i32;
    let _ = SafeArrayGetLBound(data, 1, &mut lo);
    let _ = SafeArrayGetUBound(data, 1, &mut hi);
    let stride = props.len() as i32;
    let mut i = lo;
    while i + stride - 1 <= hi && hits.len() < MAX_HITS {
        let mut name_v = zeroed();
        let mut val_v = zeroed();
        let mut rect_v = zeroed();
        let mut off_v = zeroed();
        let i0 = i;
        let i1 = i + 1;
        let i2 = i + 2;
        let i3 = i + 3;
        let _ = SafeArrayGetElement(data, &i0, &mut name_v as *mut _ as *mut c_void);
        let _ = SafeArrayGetElement(data, &i1, &mut val_v as *mut _ as *mut c_void);
        let _ = SafeArrayGetElement(data, &i2, &mut rect_v as *mut _ as *mut c_void);
        let _ = SafeArrayGetElement(data, &i3, &mut off_v as *mut _ as *mut c_void);
        if !variant_bool_true(&off_v) {
            let mut name = variant_bstr(&name_v);
            if name.is_empty() {
                name = variant_bstr(&val_v);
            }
            if let Some((left, top, width, height)) = variant_rect4(&rect_v) {
                if let Some(hit) = screen_box_to_hit(
                    name,
                    left.round() as i32,
                    top.round() as i32,
                    (left + width).round() as i32,
                    (top + height).round() as i32,
                    origin,
                    bmp_w,
                    bmp_h,
                ) {
                    push_hit(hits, hit);
                }
            }
        }
        let _ = VariantClear(&mut name_v);
        let _ = VariantClear(&mut val_v);
        let _ = VariantClear(&mut rect_v);
        let _ = VariantClear(&mut off_v);
        i += stride;
    }
    let _ = SafeArrayDestroy(data);
}

fn uia_hits(hwnd: HWND, origin: RECT, bmp_w: i32, bmp_h: i32) -> Vec<Hit> {
    use windows_sys::Win32::UI::Accessibility::{UiaNodeFromHandle, UiaNodeRelease};
    ole_init();
    let mut hits = Vec::new();
    unsafe {
        let mut root = ptr::null_mut();
        if UiaNodeFromHandle(hwnd, &mut root) < 0 || root.is_null() {
            return hits;
        }
        uia_find_batch(root, origin, bmp_w, bmp_h, &mut hits);
        if hits.len() < 4 {
            walk_uia(root, origin, bmp_w, bmp_h, &mut hits, 0);
        }
        UiaNodeRelease(root);
    }
    hits
}

fn collect_hits(hwnd: HWND, origin: RECT, bmp_w: i32, bmp_h: i32) -> Vec<Hit> {
    let mut hits = uia_hits(hwnd, origin, bmp_w, bmp_h);
    for h in child_hits_raw(hwnd, origin, bmp_w, bmp_h) {
        push_hit(&mut hits, h);
    }
    hits.truncate(MAX_HITS);
    hits
}

fn look_payload(
    jpeg: Vec<u8>,
    width: i32,
    height: i32,
    origin: RECT,
    title: &str,
    hits: Vec<Hit>,
    monitors: Option<Vec<Value>>,
) -> Value {
    let src_w = (origin.right - origin.left).max(0);
    let src_h = (origin.bottom - origin.top).max(0);
    let grid = grid_cells(width, height);
    let mut st = STATE.lock().unwrap_or_else(|e| e.into_inner());
    st.bmp_w = width;
    st.bmp_h = height;
    st.origin_x = origin.left;
    st.origin_y = origin.top;
    st.src_w = src_w;
    st.src_h = src_h;
    st.last_title = title.to_string();
    st.last_hits = hits.clone();
    drop(st);
    let hit_json: Vec<Value> = hits.iter().map(hit_json).collect();
    let mut v = json!({
        "ok": true,
        "width": width,
        "height": height,
        "origin": { "x": origin.left, "y": origin.top },
        "title": title,
        "hits": hit_json,
        "grid_px": GRID,
        "grid": grid,
        "mimeType": "image/jpeg",
        "image": format!("data:image/jpeg;base64,{}", b64(&jpeg)),
    });
    if let Some(m) = monitors {
        v["monitors"] = json!(m);
    }
    v
}

fn current_target() -> Result<HWND, ApiError> {
    let st = STATE.lock().unwrap_or_else(|e| e.into_inner());
    if st.target == 0 {
        return Err(ApiError::bad("no target-window; call target-window first"));
    }
    let h = hwnd_from_usize(st.target);
    if unsafe { windows_sys::Win32::UI::WindowsAndMessaging::IsWindow(h) } == 0 {
        return Err(ApiError::bad("target window is gone"));
    }
    Ok(h)
}

#[derive(Clone, Debug)]
struct WinInfo {
    title: String,
    class: String,
    pid: u32,
    hwnd: usize,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    client_w: i32,
    client_h: i32,
    unusable: bool,
    parked: bool,
    process_name: String,
    url: Option<String>,
}

impl WinInfo {
    fn to_json(&self) -> Value {
        let mut v = json!({
            "title": self.title,
            "class": self.class,
            "pid": self.pid,
            "hwnd": self.hwnd as u64,
            "id": self.hwnd.to_string(),
            "x": self.x,
            "y": self.y,
            "w": self.w,
            "h": self.h,
            "client_w": self.client_w,
            "client_h": self.client_h,
            "unusable": self.unusable,
            "parked": self.parked,
            "processName": self.process_name,
        });
        if let Some(ref url) = self.url {
            v["url"] = json!(url);
        }
        v
    }
}

#[derive(Debug)]
enum PickErr {
    NeedSelector,
    Empty,
    Ambiguous(Vec<WinInfo>),
    UnusableOnly(Vec<WinInfo>),
}

fn pick_target(
    all: &[WinInfo],
    title: &str,
    pid: Option<u32>,
    id: Option<u64>,
) -> Result<usize, PickErr> {
    let needle = title.trim().to_ascii_lowercase();
    if needle.is_empty() && pid.is_none() && id.is_none() {
        return Err(PickErr::NeedSelector);
    }
    let mut cands: Vec<WinInfo> = all
        .iter()
        .filter(|w| {
            if let Some(id) = id {
                if w.hwnd as u64 != id {
                    return false;
                }
            }
            if let Some(pid) = pid {
                if w.pid != pid {
                    return false;
                }
            }
            if !needle.is_empty() && !w.title.to_ascii_lowercase().contains(&needle) {
                return false;
            }
            true
        })
        .cloned()
        .collect();
    if cands.is_empty() {
        return Err(PickErr::Empty);
    }
    if id.is_some() {
        return Ok(cands[0].hwnd);
    }
    let usable: Vec<WinInfo> = cands.iter().filter(|w| !w.unusable).cloned().collect();
    match usable.len() {
        0 => Err(PickErr::UnusableOnly(cands)),
        1 => Ok(usable[0].hwnd),
        _ => {
            cands.retain(|w| !w.unusable);
            Err(PickErr::Ambiguous(cands))
        }
    }
}

fn pick_err_api(err: PickErr) -> ApiError {
    match err {
        PickErr::NeedSelector => ApiError::bad("target-window needs title, pid, or id"),
        PickErr::Empty => ApiError::bad("no visible window matches that title/pid/id"),
        PickErr::Ambiguous(cands) => ApiError {
            status: 400,
            body: json!({
                "error": "several usable windows match; pass pid or id (hwnd)",
                "candidates": cands.iter().map(WinInfo::to_json).collect::<Vec<_>>(),
            }),
        },
        PickErr::UnusableOnly(cands) => ApiError {
            status: 400,
            body: json!({
                "error": "matching windows are cloaked or too small; pass a usable pid or id",
                "candidates": cands.iter().map(WinInfo::to_json).collect::<Vec<_>>(),
            }),
        },
    }
}

struct ListAcc {
    rows: Vec<WinInfo>,
}

unsafe extern "system" fn enum_list(hwnd: HWND, lparam: LPARAM) -> BOOL {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetWindowRect, GetWindowThreadProcessId, IsWindowVisible,
    };
    let acc = lparam as *mut ListAcc;
    unsafe {
        let parked = hwnd_is_iconic(hwnd);
        if IsWindowVisible(hwnd) == 0 && !parked {
            return 1;
        }
        let class = class_of(hwnd);
        if is_shell_chrome(&class) {
            return 1;
        }
        let title = title_of(hwnd);
        if title.is_empty() {
            return 1;
        }
        let mut r = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        if GetWindowRect(hwnd, &mut r) == 0 {
            return 1;
        }
        let mut pid = 0u32;
        let _ = GetWindowThreadProcessId(hwnd, &mut pid);
        let (cw, ch) = client_size(hwnd);
        let cloaked = window_cloaked(hwnd);
        let unusable = window_unusable(r.left, r.top, cw, ch, cloaked);
        let process_name = process_image_stem(pid);
        let url = title_browser_url(&title);
        (*acc).rows.push(WinInfo {
            title,
            class,
            pid,
            hwnd: usize_from_hwnd(hwnd),
            x: r.left,
            y: r.top,
            w: r.right - r.left,
            h: r.bottom - r.top,
            client_w: cw,
            client_h: ch,
            unusable,
            parked,
            process_name,
            url,
        });
    }
    1
}

fn list_window_rows() -> Vec<WinInfo> {
    use windows_sys::Win32::UI::WindowsAndMessaging::EnumWindows;
    let mut acc = ListAcc { rows: Vec::new() };
    unsafe {
        let _ = EnumWindows(Some(enum_list), &mut acc as *mut _ as LPARAM);
    }
    acc.rows
}

fn resolve_hwnd(title: &str, pid: Option<u32>, id: Option<u64>) -> Result<HWND, ApiError> {
    let rows = list_window_rows();
    match pick_target(&rows, title, pid, id) {
        Ok(h) => {
            let hwnd = hwnd_from_usize(h);
            if unsafe { windows_sys::Win32::UI::WindowsAndMessaging::IsWindow(hwnd) } == 0 {
                Err(ApiError::bad("target window is gone"))
            } else {
                Ok(hwnd)
            }
        }
        Err(e) => Err(pick_err_api(e)),
    }
}

fn list_windows() -> ApiResult {
    let rows = list_window_rows();
    Ok(json!({
        "ok": true,
        "windows": rows.iter().map(WinInfo::to_json).collect::<Vec<_>>(),
    }))
}

fn target_window(args: &Value) -> ApiResult {
    let title = args.get("title").and_then(|x| x.as_str()).unwrap_or("");
    let pid = json_u64(args, &["pid"]).map(|n| n as u32);
    let id = json_u64(args, &["id", "hwnd"]);
    let hwnd = resolve_hwnd(title, pid, id)?;
    let mut st = STATE.lock().unwrap_or_else(|e| e.into_inner());
    st.target = usize_from_hwnd(hwnd);
    drop(st);
    let t = title_of(hwnd);
    let (cw, ch) = client_size(hwnd);
    let r = window_rect(hwnd);
    let unusable = r
        .map(|r| window_unusable(r.left, r.top, cw, ch, window_cloaked(hwnd)))
        .unwrap_or(false);
    let mut pid = 0u32;
    unsafe {
        let _ = windows_sys::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId(hwnd, &mut pid);
    }
    Ok(json!({
        "ok": true,
        "title": t,
        "hwnd": usize_from_hwnd(hwnd) as u64,
        "id": usize_from_hwnd(hwnd).to_string(),
        "pid": pid,
        "client_w": cw,
        "client_h": ch,
        "unusable": unusable,
        "class": class_of(hwnd),
    }))
}

fn look_at_window(args: &Value) -> ApiResult {
    let title = args.get("title").and_then(|x| x.as_str()).unwrap_or("");
    let pid = json_u64(args, &["pid"]).map(|n| n as u32);
    let id = json_u64(args, &["id", "hwnd"]);
    let hwnd = if title.trim().is_empty() && pid.is_none() && id.is_none() {
        current_target()?
    } else {
        let h = resolve_hwnd(title, pid, id)?;
        STATE.lock().unwrap_or_else(|e| e.into_inner()).target = usize_from_hwnd(h);
        h
    };
    with_peek(hwnd, || {
        let r = window_rect(hwnd).ok_or_else(|| ApiError::bad("GetWindowRect failed"))?;
        let w = r.right - r.left;
        let h = r.bottom - r.top;
        let (jpeg, bw, bh) = capture_rect(r.left, r.top, w, h, Some(hwnd))?;
        let hits = collect_hits(hwnd, r, bw, bh);
        Ok(look_payload(jpeg, bw, bh, r, &title_of(hwnd), hits, None))
    })
}

fn look_at_workarea() -> ApiResult {
    use windows_sys::Win32::UI::WindowsAndMessaging::{SystemParametersInfoW, SPI_GETWORKAREA};
    let mut r = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    unsafe {
        let _ = SystemParametersInfoW(SPI_GETWORKAREA, 0, &mut r as *mut RECT as *mut c_void, 0);
    }
    let w = r.right - r.left;
    let h = r.bottom - r.top;
    let (jpeg, bw, bh) = capture_rect(r.left, r.top, w, h, None)?;
    Ok(look_payload(jpeg, bw, bh, r, "work area", Vec::new(), None))
}

struct MonAcc {
    rows: Vec<Value>,
}

unsafe extern "system" fn enum_mon(
    hmon: windows_sys::Win32::Graphics::Gdi::HMONITOR,
    _hdc: windows_sys::Win32::Graphics::Gdi::HDC,
    _lprc: *mut RECT,
    lparam: LPARAM,
) -> BOOL {
    use windows_sys::Win32::Graphics::Gdi::{GetMonitorInfoW, MONITORINFO};
    use windows_sys::Win32::UI::WindowsAndMessaging::MONITORINFOF_PRIMARY;
    let acc = lparam as *mut MonAcc;
    let mut info: MONITORINFO = unsafe { zeroed() };
    info.cbSize = size_of::<MONITORINFO>() as u32;
    unsafe {
        if GetMonitorInfoW(hmon, &mut info) != 0 {
            let r = info.rcMonitor;
            (*acc).rows.push(json!({
                "x": r.left,
                "y": r.top,
                "w": r.right - r.left,
                "h": r.bottom - r.top,
                "primary": (info.dwFlags & MONITORINFOF_PRIMARY) != 0,
            }));
        }
    }
    1
}

fn look_at_desktop() -> ApiResult {
    use windows_sys::Win32::Graphics::Gdi::EnumDisplayMonitors;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
    };
    let x = unsafe { GetSystemMetrics(SM_XVIRTUALSCREEN) };
    let y = unsafe { GetSystemMetrics(SM_YVIRTUALSCREEN) };
    let w = unsafe { GetSystemMetrics(SM_CXVIRTUALSCREEN) };
    let h = unsafe { GetSystemMetrics(SM_CYVIRTUALSCREEN) };
    let r = RECT {
        left: x,
        top: y,
        right: x + w,
        bottom: y + h,
    };
    let (jpeg, bw, bh) = capture_rect(x, y, w, h, None)?;
    let mut acc = MonAcc { rows: Vec::new() };
    unsafe {
        let _ = EnumDisplayMonitors(ptr::null_mut(), ptr::null(), Some(enum_mon), &mut acc as *mut _ as LPARAM);
    }
    Ok(look_payload(jpeg, bw, bh, r, "desktop", Vec::new(), Some(acc.rows)))
}

fn bitmap_to_screen(x: i32, y: i32) -> (i32, i32) {
    let st = STATE.lock().unwrap_or_else(|e| e.into_inner());
    map_bitmap_to_screen(
        x,
        y,
        st.origin_x,
        st.origin_y,
        st.bmp_w,
        st.bmp_h,
        st.src_w,
        st.src_h,
    )
}

fn screen_to_client(hwnd: HWND, sx: i32, sy: i32) -> POINT {
    use windows_sys::Win32::Graphics::Gdi::ScreenToClient;
    let mut p = POINT { x: sx, y: sy };
    unsafe {
        let _ = ScreenToClient(hwnd, &mut p);
    }
    p
}

fn post_click(hwnd: HWND, client: POINT) {
    use windows_sys::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetWindowThreadProcessId, PostMessageW, WM_LBUTTONDOWN, WM_LBUTTONUP,
    };
    let lp = ((client.y as u32) << 16) | (client.x as u32 & 0xFFFF);
    unsafe {
        let our = GetCurrentThreadId();
        let mut pid = 0u32;
        let their = GetWindowThreadProcessId(hwnd, &mut pid);
        if their != 0 && their != our {
            let _ = AttachThreadInput(our, their, 1);
        }
        let _ = PostMessageW(hwnd, WM_LBUTTONDOWN, 1usize, lp as isize);
        let _ = PostMessageW(hwnd, WM_LBUTTONUP, 0, lp as isize);
        if their != 0 && their != our {
            let _ = AttachThreadInput(our, their, 0);
        }
    }
}

fn window_fingerprint(hwnd: HWND) -> Option<u64> {
    let r = window_rect(hwnd)?;
    let w = r.right - r.left;
    let h = r.bottom - r.top;
    if w <= 0 || h <= 0 {
        return None;
    }
    let (jpeg, _, _) = capture_rect(r.left, r.top, w, h, Some(hwnd)).ok()?;
    let title = title_of(hwnd);
    let (cw, ch) = client_size(hwnd);
    Some(fnv64(&jpeg) ^ fnv64(title.as_bytes()) ^ ((cw as u64) << 32) ^ (ch as u64))
}

fn window_click(args: &Value) -> ApiResult {
    let hwnd = current_target()?;
    if class_of(hwnd) == CLASS_HOST {
        return Err(ApiError::bad("refusing to drive StarDustHost"));
    }
    with_peek(hwnd, || {
        let mut x = args.get("x").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
        let mut y = args.get("y").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
        let mut on_hit = false;
        if let Some(name) = args.get("name").and_then(|v| v.as_str()) {
            if !name.is_empty() {
                let r = window_rect(hwnd).ok_or_else(|| ApiError::bad("GetWindowRect failed"))?;
                let st = STATE.lock().unwrap_or_else(|e| e.into_inner());
                let bw = st.bmp_w.max(1);
                let bh = st.bmp_h.max(1);
                let mut hits = st.last_hits.clone();
                drop(st);
                if hits.is_empty() {
                    hits = collect_hits(hwnd, r, bw, bh);
                }
                let needle = name.to_ascii_lowercase();
                let hit = hits.iter().find(|h| h.name.to_ascii_lowercase().contains(&needle));
                match hit {
                    Some(h) => {
                        x = h.x + h.w / 2;
                        y = h.y + h.h / 2;
                        on_hit = true;
                    }
                    None => {
                        let fresh = collect_hits(hwnd, r, bw, bh);
                        let hit = fresh
                            .iter()
                            .find(|h| h.name.to_ascii_lowercase().contains(&needle));
                        match hit {
                            Some(h) => {
                                x = h.x + h.w / 2;
                                y = h.y + h.h / 2;
                                on_hit = true;
                            }
                            None => return Err(ApiError::bad("no hit-list control matches that name")),
                        }
                    }
                }
            }
        }
        if !on_hit {
            let st = STATE.lock().unwrap_or_else(|e| e.into_inner());
            on_hit = point_in_hits(x, y, &st.last_hits);
        }
        let before = if on_hit {
            None
        } else {
            window_fingerprint(hwnd)
        };
        let (sx, sy) = bitmap_to_screen(x, y);
        let client = screen_to_client(hwnd, sx, sy);
        post_click(hwnd, client);
        if !on_hit {
            std::thread::sleep(Duration::from_millis(80));
            let after = window_fingerprint(hwnd);
            let changed = match (before, after) {
                (Some(a), Some(b)) => a != b,
                _ => true,
            };
            if coord_click_miss(on_hit, changed) {
                return Err(ApiError::bad(
                    "miss: coordinate click did not change the target UI",
                ));
            }
        }
        Ok(json!({ "ok": true, "x": x, "y": y }))
    })
}

fn key_event(vk: u16, up: bool) {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP,
    };
    let mut inp = INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: if up { KEYEVENTF_KEYUP } else { 0 },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    unsafe {
        let _ = SendInput(1, &mut inp, size_of::<INPUT>() as i32);
    }
}

fn tap_vk(vk: u16) {
    key_event(vk, false);
    key_event(vk, true);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ChordKind {
    Tap(u16),
    Ctrl(u16),
    Shift(u16),
}

fn parse_chord(spec: &str) -> Option<ChordKind> {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        VK_BACK, VK_DELETE, VK_ESCAPE, VK_RETURN, VK_TAB,
    };
    let s = spec.trim();
    if s.eq_ignore_ascii_case("Enter") || s.eq_ignore_ascii_case("Return") {
        return Some(ChordKind::Tap(VK_RETURN));
    }
    if s.eq_ignore_ascii_case("Escape") || s.eq_ignore_ascii_case("Esc") {
        return Some(ChordKind::Tap(VK_ESCAPE));
    }
    if s.eq_ignore_ascii_case("Tab") {
        return Some(ChordKind::Tap(VK_TAB));
    }
    if s.eq_ignore_ascii_case("Shift+Tab") {
        return Some(ChordKind::Shift(VK_TAB));
    }
    if s.eq_ignore_ascii_case("Backspace") || s.eq_ignore_ascii_case("Bksp") {
        return Some(ChordKind::Tap(VK_BACK));
    }
    if s.eq_ignore_ascii_case("Delete") || s.eq_ignore_ascii_case("Del") {
        return Some(ChordKind::Tap(VK_DELETE));
    }
    let lower = s.to_ascii_lowercase();
    if let Some(rest) = lower.strip_prefix("ctrl+") {
        if rest.len() == 1 {
            let ch = rest.chars().next()?;
            if ch.is_ascii_alphabetic() {
                return Some(ChordKind::Ctrl(ch.to_ascii_uppercase() as u16));
            }
        }
        if rest == "enter" || rest == "return" {
            return Some(ChordKind::Ctrl(VK_RETURN));
        }
    }
    None
}

fn play_chord(kind: ChordKind) {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{VK_CONTROL, VK_SHIFT};
    match kind {
        ChordKind::Tap(vk) => tap_vk(vk),
        ChordKind::Ctrl(vk) => {
            key_event(VK_CONTROL, false);
            tap_vk(vk);
            key_event(VK_CONTROL, true);
        }
        ChordKind::Shift(vk) => {
            key_event(VK_SHIFT, false);
            tap_vk(vk);
            key_event(VK_SHIFT, true);
        }
    }
}

fn type_chord(spec: &str) -> bool {
    let Some(kind) = parse_chord(spec) else {
        return false;
    };
    play_chord(kind);
    true
}

fn composer_guard(args: &Value, run_args: &Value) -> Result<(), ApiError> {
    let force = json_force(args) || json_force(run_args);
    let Some(expect) = expect_contact(args, run_args) else {
        return Ok(());
    };
    let st = STATE.lock().unwrap_or_else(|e| e.into_inner());
    let names: Vec<String> = st.last_hits.iter().map(|h| h.name.clone()).collect();
    let ok = composer_allows(&expect, force, &st.last_title, &names);
    drop(st);
    if ok {
        Ok(())
    } else {
        Err(ApiError::bad(format!(
            "refusing window-type: last look header/name does not match `{expect}` (set force: true to override)"
        )))
    }
}

fn window_type(args: &Value) -> ApiResult {
    use windows_sys::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_CHAR};
    let hwnd = current_target()?;
    if class_of(hwnd) == CLASS_HOST {
        return Err(ApiError::bad("refusing to drive StarDustHost"));
    }
    composer_guard(args, args)?;
    let text = args.get("text").and_then(|v| v.as_str()).unwrap_or("");
    let chord = args.get("chord").and_then(|v| v.as_str()).unwrap_or("");
    with_peek(hwnd, || {
        if !chord.is_empty() && !type_chord(chord) {
            return Err(ApiError::bad("unsupported chord"));
        }
        if !text.is_empty() && parse_chord(text).is_none() {
            for ch in text.chars() {
                unsafe {
                    let _ = PostMessageW(hwnd, WM_CHAR, ch as usize, 0);
                }
            }
        } else if !text.is_empty() {
            let _ = type_chord(text);
        }
        Ok(json!({ "ok": true }))
    })
}

fn make_hdrop(path: &Path) -> Result<windows_sys::Win32::Foundation::HGLOBAL, ApiError> {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GHND};
    use windows_sys::Win32::UI::Shell::DROPFILES;
    let wpath: Vec<u16> = path
        .to_string_lossy()
        .encode_utf16()
        .chain(std::iter::once(0))
        .chain(std::iter::once(0))
        .collect();
    let header = size_of::<DROPFILES>();
    let bytes = header + wpath.len() * 2;
    let hg = unsafe { GlobalAlloc(GHND, bytes) };
    if hg.is_null() {
        return Err(ApiError::internal("GlobalAlloc HDROP failed"));
    }
    let p = unsafe { GlobalLock(hg) } as *mut u8;
    if p.is_null() {
        unsafe {
            let _ = windows_sys::Win32::Foundation::GlobalFree(hg);
        }
        return Err(ApiError::internal("GlobalLock HDROP failed"));
    }
    unsafe {
        let df = p as *mut DROPFILES;
        (*df).pFiles = header as u32;
        (*df).pt = POINT { x: 0, y: 0 };
        (*df).fNC = 0;
        (*df).fWide = 1;
        std::ptr::copy_nonoverlapping(wpath.as_ptr() as *const u8, p.add(header), wpath.len() * 2);
        let _ = GlobalUnlock(hg);
    }
    Ok(hg)
}

fn drop_file(args: &Value) -> ApiResult {
    use windows_sys::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
    };
    use windows_sys::Win32::System::Ole::CF_HDROP;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SendMessageTimeoutW, SMTO_ABORTIFHUNG, WM_DROPFILES,
    };
    let hwnd = current_target()?;
    if class_of(hwnd) == CLASS_HOST {
        return Err(ApiError::bad("refusing to drive StarDustHost"));
    }
    let path_s = args.get("path").and_then(|v| v.as_str()).unwrap_or("");
    let path = parse_abs_file(path_s)?;
    with_peek(hwnd, || {
        let hdrop = make_hdrop(&path)?;
        let hdrop2 = make_hdrop(&path)?;
        let mut result = 0usize;
        unsafe {
            let _ = SendMessageTimeoutW(
                hwnd,
                WM_DROPFILES,
                hdrop as usize,
                0,
                SMTO_ABORTIFHUNG,
                1500,
                &mut result,
            );
            if OpenClipboard(ptr::null_mut()) != 0 {
                let _ = EmptyClipboard();
                let _ = SetClipboardData(CF_HDROP as u32, hdrop2);
                let _ = CloseClipboard();
            } else {
                let _ = windows_sys::Win32::Foundation::GlobalFree(hdrop2);
            }
        }
        type_chord("Ctrl+V");
        Ok(json!({ "ok": true, "path": path.display().to_string() }))
    })
}

fn surprise_dialog(target: HWND) -> Option<HWND> {
    use windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
    let fg = unsafe { GetForegroundWindow() };
    if fg.is_null() || fg == target {
        return None;
    }
    let cls = class_of(fg);
    if cls == CLASS_HOST || is_shell_chrome(&cls) {
        return None;
    }
    Some(fg)
}

fn look_hwnd(hwnd: HWND) -> ApiResult {
    let r = window_rect(hwnd).ok_or_else(|| ApiError::bad("GetWindowRect failed"))?;
    let w = r.right - r.left;
    let h = r.bottom - r.top;
    let (jpeg, bw, bh) = capture_rect(r.left, r.top, w, h, Some(hwnd))?;
    let hits = collect_hits(hwnd, r, bw, bh);
    Ok(look_payload(jpeg, bw, bh, r, &title_of(hwnd), hits, None))
}

fn dispatch(op: &str, args: &Value) -> ApiResult {
    match op {
        "list-windows" => list_windows(),
        "target-window" => target_window(args),
        "look-at-window" => look_at_window(args),
        "look-at-workarea" => look_at_workarea(),
        "look-at-desktop" => look_at_desktop(),
        "window-click" => window_click(args),
        "window-type" => window_type(args),
        "drop-file" => drop_file(args),
        "peek-slot" => set_peek_slot(args),
        other => Err(ApiError::bad(format!("unknown drive op {other}"))),
    }
}

fn run_ui(args: &Value) -> ApiResult {
    let steps = args
        .get("steps")
        .and_then(|v| v.as_array())
        .ok_or_else(|| ApiError::bad("run-ui requires steps[]"))?;
    if steps.len() > MAX_RUN_STEPS {
        return Err(ApiError::bad(format!(
            "run-ui max {MAX_RUN_STEPS} steps; split the task"
        )));
    }
    let _hold = RunPeekHold::enter();
    let mut checkpoint: Option<Value> = None;
    for (i, step) in steps.iter().enumerate() {
        let op = step.get("op").and_then(|v| v.as_str()).unwrap_or("");
        if is_type_or_enter(step) {
            if let Err(e) = composer_guard(step, args) {
                let look = current_target()
                    .ok()
                    .and_then(|h| look_hwnd(h).ok())
                    .unwrap_or_else(|| e.body.clone());
                return Ok(json!({
                    "ok": false,
                    "error": e.body.get("error").and_then(|x| x.as_str()).unwrap_or("composer guard"),
                    "stopped_at": i,
                    "image": look.get("image").cloned().unwrap_or(Value::Null),
                    "width": look.get("width"),
                    "height": look.get("height"),
                    "title": look.get("title"),
                    "hits": look.get("hits"),
                    "grid": look.get("grid"),
                    "mimeType": "image/jpeg",
                }));
            }
        }
        match dispatch(op, step) {
            Ok(v) => {
                if op.starts_with("look-at-") {
                    checkpoint = Some(v.clone());
                }
                if op == "window-click" || op == "window-type" || op == "drop-file" {
                    if let Ok(target) = current_target() {
                        if let Some(dlg) = surprise_dialog(target) {
                            let look = look_hwnd(dlg).unwrap_or_else(|e| e.body);
                            return Ok(json!({
                                "ok": false,
                                "error": "stopped: new dialog or focus change",
                                "stopped_at": i,
                                "image": look.get("image").cloned().unwrap_or(Value::Null),
                                "width": look.get("width"),
                                "height": look.get("height"),
                                "title": look.get("title"),
                                "mimeType": "image/jpeg",
                            }));
                        }
                    }
                }
            }
            Err(e) => {
                let look = current_target()
                    .ok()
                    .and_then(|h| look_hwnd(h).ok())
                    .unwrap_or_else(|| e.body.clone());
                return Ok(json!({
                    "ok": false,
                    "error": e.body.get("error").and_then(|x| x.as_str()).unwrap_or("drive step failed"),
                    "stopped_at": i,
                    "image": look.get("image").cloned().unwrap_or(Value::Null),
                    "width": look.get("width"),
                    "height": look.get("height"),
                    "title": look.get("title"),
                    "hits": look.get("hits"),
                    "grid": look.get("grid"),
                    "mimeType": "image/jpeg",
                }));
            }
        }
    }
    let final_look = if let Ok(h) = current_target() {
        look_hwnd(h).ok()
    } else {
        None
    };
    let mut out = json!({ "ok": true });
    if let Some(look) = final_look {
        out["image"] = look.get("image").cloned().unwrap_or(Value::Null);
        out["width"] = look.get("width").cloned().unwrap_or(Value::Null);
        out["height"] = look.get("height").cloned().unwrap_or(Value::Null);
        out["title"] = look.get("title").cloned().unwrap_or(Value::Null);
        out["hits"] = look.get("hits").cloned().unwrap_or(json!([]));
        out["grid"] = look.get("grid").cloned().unwrap_or(json!([]));
        out["grid_px"] = look.get("grid_px").cloned().unwrap_or(json!(GRID));
        out["mimeType"] = json!("image/jpeg");
    }
    if let Some(cp) = checkpoint {
        out["checkpoint"] = cp.get("image").cloned().unwrap_or(Value::Null);
    }
    Ok(out)
}

pub fn command(v: &Value) -> ApiResult {
    ole_init();
    let op = v.get("op").and_then(|x| x.as_str()).unwrap_or("");
    let args = v.get("args").cloned().unwrap_or_else(|| json!({}));
    if op == "run-ui" {
        return run_ui(&args);
    }
    dispatch(op, &args)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn forbidden_path_rejects_appdata_and_agent_cwd() {
        assert!(forbidden_path(Path::new(
            r"C:\Users\x\AppData\Roaming\StarDust\ui-layout.json"
        )));
        assert!(forbidden_path(Path::new(r"C:\Users\x\Application Data\foo")));
        assert!(forbidden_path(
            &PathBuf::from(std::env::temp_dir()).join("stardust-ws-1-2").join("a.pdf")
        ));
        assert!(!forbidden_path(Path::new(r"C:\Users\x\Desktop\note.pdf")));
    }

    #[test]
    fn run_ui_rejects_too_many_steps() {
        let mut steps = Vec::new();
        for _ in 0..21 {
            steps.push(json!({ "op": "list-windows" }));
        }
        let err = run_ui(&json!({ "steps": steps })).expect_err("cap");
        let msg = err.body["error"].as_str().unwrap_or("");
        assert!(msg.contains("max"), "{msg}");
    }

    #[test]
    fn run_ui_stops_on_missing_target() {
        let v = run_ui(&json!({
            "steps": [{ "op": "window-click", "x": 1, "y": 1 }]
        }))
        .expect("ok wrapper");
        assert_eq!(v["ok"], false);
        assert_eq!(v["stopped_at"], 0);
        let err = v["error"].as_str().unwrap_or("");
        assert!(err.contains("target"), "{err}");
    }

    #[test]
    fn jpeg_encode_small_bitmap() {
        use windows_sys::Win32::Graphics::Gdi::{
            CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, ReleaseDC,
            SelectObject, PatBlt, BLACKNESS,
        };
        unsafe {
            let hdc = GetDC(ptr::null_mut());
            assert!(!hdc.is_null());
            let mem = CreateCompatibleDC(hdc);
            let hbmp = CreateCompatibleBitmap(hdc, 16, 16);
            let old = SelectObject(mem, hbmp);
            let _ = PatBlt(mem, 0, 0, 16, 16, BLACKNESS);
            let _ = SelectObject(mem, old);
            let bytes = encode_hbitmap(hbmp).expect("jpeg");
            let _ = DeleteObject(hbmp);
            let _ = DeleteDC(mem);
            let _ = ReleaseDC(ptr::null_mut(), hdc);
            assert!(bytes.len() > 32, "jpeg too small");
            assert_eq!(bytes[0], 0xFF);
            assert_eq!(bytes[1], 0xD8);
        }
    }

    fn sample_windows() -> Vec<WinInfo> {
        vec![
            WinInfo {
                title: "WhatsApp".into(),
                class: "ApplicationFrameWindow".into(),
                pid: 111,
                hwnd: 10,
                x: 540,
                y: 0,
                w: 1000,
                h: 1028,
                client_w: 990,
                client_h: 1000,
                unusable: false,
                parked: false,
                process_name: String::new(),
                url: None,
            },
            WinInfo {
                title: "(3) WhatsApp".into(),
                class: "Chrome_WidgetWin_1".into(),
                pid: 222,
                hwnd: 20,
                x: 0,
                y: 0,
                w: 982,
                h: 1018,
                client_w: 970,
                client_h: 980,
                unusable: false,
                parked: false,
                process_name: String::new(),
                url: None,
            },
            WinInfo {
                title: "WhatsApp".into(),
                class: "ApplicationFrameWindow".into(),
                pid: 111,
                hwnd: 30,
                x: -32000,
                y: -32000,
                w: 199,
                h: 34,
                client_w: 199,
                client_h: 34,
                unusable: true,
                parked: false,
                process_name: String::new(),
                url: None,
            },
        ]
    }

    #[test]
    fn cloaked_or_tiny_is_unusable() {
        assert!(window_unusable(-32000, -32000, 199, 34, false));
        assert!(window_unusable(0, 0, 199, 34, false));
        assert!(window_unusable(100, 100, 800, 600, true));
        assert!(!window_unusable(540, 0, 982, 1000, false));
    }

    #[test]
    fn title_ambiguity_errors_without_pid_or_id() {
        let err = pick_target(&sample_windows(), "WhatsApp", None, None).unwrap_err();
        match err {
            PickErr::Ambiguous(cands) => {
                assert!(cands.len() >= 2, "{}", cands.len());
                assert!(cands.iter().all(|w| !w.unusable));
            }
            other => panic!("expected ambiguous, got {other:?}"),
        }
    }

    #[test]
    fn cloaked_title_only_rejected() {
        let only = vec![sample_windows()[2].clone()];
        let err = pick_target(&only, "WhatsApp", None, None).unwrap_err();
        match err {
            PickErr::UnusableOnly(cands) => assert_eq!(cands.len(), 1),
            other => panic!("expected unusable-only, got {other:?}"),
        }
    }

    #[test]
    fn single_usable_after_dropping_cloaked_binds() {
        let rows = vec![sample_windows()[0].clone(), sample_windows()[2].clone()];
        let hwnd = pick_target(&rows, "WhatsApp", None, None).expect("one usable");
        assert_eq!(hwnd, 10);
    }

    #[test]
    fn pid_or_id_selects_among_title_matches() {
        assert_eq!(
            pick_target(&sample_windows(), "WhatsApp", Some(222), None).unwrap(),
            20
        );
        assert_eq!(
            pick_target(&sample_windows(), "", None, Some(30)).unwrap(),
            30
        );
    }

    #[test]
    fn extra_chords_are_accepted() {
        assert!(parse_chord("Tab").is_some());
        assert!(parse_chord("Shift+Tab").is_some());
        assert!(parse_chord("Backspace").is_some());
        assert!(parse_chord("Delete").is_some());
        assert!(parse_chord("Ctrl+A").is_some());
        assert!(parse_chord("Ctrl+L").is_some());
        assert!(parse_chord("Ctrl+Enter").is_some());
        assert!(parse_chord("Enter").is_some());
        assert!(parse_chord("Escape").is_some());
        assert!(parse_chord("Ctrl+V").is_some());
        assert!(parse_chord("Ctrl+F").is_some());
        assert!(parse_chord("F1").is_none());
        assert!(parse_chord("Alt+Tab").is_none());
    }

    #[test]
    fn miss_on_no_change_without_hit_box() {
        assert!(coord_click_miss(false, false));
        assert!(!coord_click_miss(true, false));
        assert!(!coord_click_miss(false, true));
        assert!(!coord_click_miss(true, true));
    }

    #[test]
    fn bitmap_to_screen_scales_like_hit_boxes() {
        assert_eq!(
            map_bitmap_to_screen(400, 200, 10, 20, 800, 400, 1600, 800),
            (10 + 800, 20 + 400)
        );
        assert_eq!(map_bitmap_to_screen(5, 5, 100, 200, 50, 50, 50, 50), (105, 205));
    }

    #[test]
    fn run_ui_composer_guard_without_hwnd() {
        {
            let mut st = STATE.lock().unwrap_or_else(|e| e.into_inner());
            st.last_title = "MATH Course B range".into();
            st.last_hits.clear();
        }
        let v = run_ui(&json!({
            "contact": "Francis",
            "steps": [{ "op": "window-type", "text": "hi" }]
        }))
        .expect("wrapper");
        assert_eq!(v["ok"], false);
        let err = v["error"].as_str().unwrap_or("");
        assert!(err.contains("header") || err.contains("contact") || err.contains("match"), "{err}");
        {
            let mut st = STATE.lock().unwrap_or_else(|e| e.into_inner());
            st.last_title = "(4) WhatsApp".into();
            st.last_hits = vec![Hit {
                name: "Francis".into(),
                x: 10,
                y: 10,
                w: 80,
                h: 20,
            }];
        }
        let v = run_ui(&json!({
            "contact": "Francis",
            "steps": [{ "op": "window-type", "text": "hi" }]
        }))
        .expect("wrapper");
        assert_eq!(v["ok"], false);
        let err = v["error"].as_str().unwrap_or("");
        assert!(err.contains("target"), "{err}");
    }

    #[test]
    fn peek_slot() {
        let v = set_peek_slot(&json!({ "x": 540, "y": 80, "w": 800, "h": 600 })).expect("slot");
        assert_eq!(v["ok"], true);
        assert_eq!(v["x"], 540);
        assert_eq!(v["y"], 80);
        assert_eq!(v["w"], 800);
        assert_eq!(v["h"], 600);
        {
            let st = STATE.lock().unwrap_or_else(|e| e.into_inner());
            assert_eq!(st.peek_x, 540);
            assert_eq!(st.peek_y, 80);
            assert_eq!(st.peek_w, 800);
            assert_eq!(st.peek_h, 600);
        }
        let err = set_peek_slot(&json!({ "x": 1, "y": 1, "w": 4, "h": 4 })).expect_err("tiny");
        let msg = err.body["error"].as_str().unwrap_or("");
        assert!(msg.contains("too small"), "{msg}");
        let missing = set_peek_slot(&json!({ "x": 10, "y": 10 })).expect_err("missing");
        let msg = missing.body["error"].as_str().unwrap_or("");
        assert!(msg.contains("peek-slot needs"), "{msg}");
    }

    #[test]
    fn fit_into_slot_shrinks_if_larger() {
        assert_eq!(fit_into_slot(1000, 1028, 800, 600), (800, 600));
        assert_eq!(fit_into_slot(400, 300, 800, 600), (400, 300));
        assert_eq!(fit_into_slot(800, 1000, 800, 600), (800, 600));
    }

    #[test]
    fn should_restore_prev_fg() {
        assert!(
            !super::should_restore_prev_fg(2, 2, 1, true),
            "never restore when prev_is_host"
        );
        assert!(
            !super::should_restore_prev_fg(1, 2, 1, true),
            "never restore StarDustHost"
        );
        assert!(
            super::should_restore_prev_fg(2, 2, 3, false),
            "restore when still on target and prev is another app"
        );
        assert!(
            !super::should_restore_prev_fg(4, 2, 3, false),
            "no restore when user switched"
        );
        assert!(!super::should_restore_prev_fg(2, 2, 0, false), "no restore when prev=0");
    }

    #[test]
    fn drag_away_from_dock_is_undock() {
        assert!(
            drag_away_from_dock(523, 80, 540, 80),
            "origin left past 16px is undock"
        );
        assert!(
            drag_away_from_dock(540, 97, 540, 80),
            "origin down past 16px is undock"
        );
        assert!(!drag_away_from_dock(540, 80, 540, 80), "origin at slot stays docked");
        assert!(
            !drag_away_from_dock(548, 80, 540, 80),
            "origin within 8px stays docked"
        );
    }

    #[test]
    fn list_includes_parked() {
        let mut rows = sample_windows();
        rows[2].parked = true;
        let json = rows[2].to_json();
        assert_eq!(json["parked"], true);
        assert_eq!(json["processName"], "");
        assert!(json.get("url").is_none());
        assert_eq!(sample_windows()[0].to_json()["parked"], false);
        assert_eq!(
            pick_target(&rows, "", None, Some(30)).unwrap(),
            30,
            "pid/id still binds a parked row"
        );
    }

    #[test]
    fn title_browser_url_strips_browser_suffix() {
        assert_eq!(
            title_browser_url("https://blackboard.cuhk.edu.hk - Google Chrome").as_deref(),
            Some("https://blackboard.cuhk.edu.hk")
        );
        assert_eq!(
            title_browser_url("MATH3270 - Microsoft Edge").as_deref(),
            Some("MATH3270")
        );
        assert_eq!(
            title_browser_url("GitHub - Mozilla Firefox").as_deref(),
            Some("GitHub")
        );
        assert_eq!(title_browser_url("Google Chrome"), None);
        assert_eq!(title_browser_url("Microsoft Edge"), None);
        assert_eq!(title_browser_url("Mozilla Firefox"), None);
        assert_eq!(title_browser_url("Google Chrome - Google Chrome"), None);
        assert_eq!(title_browser_url("WhatsApp"), None);
    }

    #[test]
    fn title_only_still_rejects_cloaked_parked_strip() {
        let mut only = sample_windows()[2].clone();
        only.parked = true;
        let err = pick_target(&[only], "WhatsApp", None, None).unwrap_err();
        match err {
            PickErr::UnusableOnly(cands) => {
                assert_eq!(cands.len(), 1);
                assert!(cands[0].parked);
                assert!(cands[0].unusable);
            }
            other => panic!("expected unusable-only, got {other:?}"),
        }
    }
}
