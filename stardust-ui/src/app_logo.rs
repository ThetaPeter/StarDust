//! Optional file `logo` (PNG data URL) from an installed-app icon source.

#[cfg(windows)]
use std::collections::HashMap;
use std::path::Path;
#[cfg(windows)]
use std::sync::Mutex;

const SHELL_MODULES: &[&str] = &[
    "shell32.dll",
    "imageres.dll",
    "imagesp1.dll",
    "moricons.dll",
    "zipfldr.dll",
    "explorer.exe",
    "notepad.exe",
    "wordpad.exe",
];

/// True when SHGetFileInfoW's icon-source path is Windows itself (star, no logo).
pub fn is_windows_icon(icon_source: &str) -> bool {
    let n = normalize_icon_path(icon_source);
    if n.is_empty() {
        return false;
    }
    let file = file_name_of(&n);
    if SHELL_MODULES.iter().any(|m| file.eq_ignore_ascii_case(m)) {
        return true;
    }
    if n.contains(r"\systemapps\") {
        return true;
    }
    if let Some(pkg) = windowsapps_package(&n) {
        if pkg.starts_with("microsoft.windows.")
            || pkg.starts_with("microsoft.microsoftedge")
            || pkg.starts_with("microsoft.edge.")
        {
            return true;
        }
    }
    is_under_windows_directory(&n)
}

/// Folders and `*.txt` (any case) never get a logo.
pub fn skips_app_logo(kind: &str, name: &str) -> bool {
    kind != "file" || name.to_ascii_lowercase().ends_with(".txt")
}

/// PNG data URL for an installed-app icon, or `None` if Windows/missing/fail.
pub fn logo_png_data_url(path: &Path, mtime: u64) -> Option<String> {
    #[cfg(windows)]
    {
        logo_data_url(path, mtime)
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        let _ = mtime;
        None
    }
}

fn normalize_icon_path(p: &str) -> String {
    let mut s = p.replace('/', "\\").to_ascii_lowercase();
    if let Some(rest) = s.strip_prefix(r"\\?\") {
        s = if let Some(unc) = rest.strip_prefix("unc\\") {
            format!(r"\\{unc}")
        } else {
            rest.to_string()
        };
    }
    s
}

fn file_name_of(n: &str) -> &str {
    let raw = n.rsplit('\\').next().filter(|s| !s.is_empty()).unwrap_or(n);
    match raw.find(',') {
        Some(i) if i > 0 => &raw[..i],
        _ => raw,
    }
}

fn windowsapps_package(n: &str) -> Option<&str> {
    let marker = r"\windowsapps\";
    let i = n.find(marker)?;
    let rest = &n[i + marker.len()..];
    let end = rest.find('\\').unwrap_or(rest.len());
    let pkg = &rest[..end];
    if pkg.is_empty() {
        None
    } else {
        Some(pkg)
    }
}

fn is_under_windows_directory(n: &str) -> bool {
    if n.starts_with("%systemroot%\\")
        || n.starts_with("%windir%\\")
        || n == "%systemroot%"
        || n == "%windir%"
    {
        return true;
    }
    let p = n.trim_start_matches('\\');
    let mut parts = p.split('\\').filter(|s| !s.is_empty());
    let Some(first) = parts.next() else {
        return false;
    };
    let win = if first.len() == 2 && first.as_bytes().get(1) == Some(&b':') {
        match parts.next() {
            Some(c) => c,
            None => return false,
        }
    } else {
        first
    };
    win == "windows"
}

#[cfg(windows)]
fn path_key(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(windows)]
fn logo_data_url(path: &Path, mtime: u64) -> Option<String> {
    ensure_com();
    let (loc, index) = match file_icon_source(path) {
        Some(v) => v,
        None => {
            cache_put(path, mtime, "", None);
            return None;
        }
    };
    if let Some(hit) = cache_get(path, mtime, &loc) {
        return hit;
    }
    let val = if is_windows_icon(&loc) {
        None
    } else {
        extract_logo_png(index)
    };
    cache_put(path, mtime, &loc, val.clone());
    val
}

#[cfg(windows)]
type LogoKey = (String, u64, String);

#[cfg(windows)]
static LOGO_CACHE: Mutex<Option<HashMap<LogoKey, Option<String>>>> = Mutex::new(None);

#[cfg(windows)]
static GDIP_TOKEN: Mutex<Option<usize>> = Mutex::new(None);

#[cfg(windows)]
fn cache_lock() -> std::sync::MutexGuard<'static, Option<HashMap<LogoKey, Option<String>>>> {
    LOGO_CACHE.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(windows)]
fn cache_get(path: &Path, mtime: u64, loc: &str) -> Option<Option<String>> {
    let key = (path_key(path), mtime, loc.to_string());
    cache_lock()
        .get_or_insert_with(HashMap::new)
        .get(&key)
        .cloned()
}

#[cfg(windows)]
fn cache_put(path: &Path, mtime: u64, loc: &str, val: Option<String>) {
    let key = (path_key(path), mtime, loc.to_string());
    cache_lock()
        .get_or_insert_with(HashMap::new)
        .insert(key, val);
}

#[cfg(windows)]
fn ensure_com() {
    use windows_sys::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
    unsafe {
        let _ = CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED as u32);
    }
}

#[cfg(windows)]
fn file_icon_source(path: &Path) -> Option<(String, i32)> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::Shell::{
        SHGetFileInfoW, SHFILEINFOW, SHGFI_ICONLOCATION, SHGFI_SYSICONINDEX,
    };
    let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    wide.push(0);
    let mut sfi = SHFILEINFOW {
        hIcon: std::ptr::null_mut(),
        iIcon: 0,
        dwAttributes: 0,
        szDisplayName: [0; 260],
        szTypeName: [0; 80],
    };
    let flags = SHGFI_ICONLOCATION | SHGFI_SYSICONINDEX;
    let r = unsafe {
        SHGetFileInfoW(
            wide.as_ptr(),
            0,
            &mut sfi,
            std::mem::size_of::<SHFILEINFOW>() as u32,
            flags,
        )
    };
    if r == 0 {
        return None;
    }
    let loc = utf16_z(&sfi.szDisplayName);
    if loc.is_empty() {
        return None;
    }
    Some((loc, sfi.iIcon))
}

#[cfg(windows)]
fn utf16_z(buf: &[u16]) -> String {
    let n = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..n])
}

#[cfg(windows)]
fn extract_logo_png(index: i32) -> Option<String> {
    use windows_sys::Win32::UI::Shell::{SHIL_JUMBO, SHIL_LARGE};
    use windows_sys::Win32::UI::WindowsAndMessaging::DestroyIcon;
    if !gdip_start() {
        return None;
    }
    for shil in [SHIL_JUMBO as i32, SHIL_LARGE as i32] {
        let Some(hicon) = image_list_icon(shil, index) else {
            continue;
        };
        let url = hicon_png_data_url(hicon);
        unsafe {
            let _ = DestroyIcon(hicon);
        }
        if url.is_some() {
            return url;
        }
    }
    None
}

#[cfg(windows)]
fn image_list_icon(
    shil: i32,
    index: i32,
) -> Option<windows_sys::Win32::UI::WindowsAndMessaging::HICON> {
    use std::ffi::c_void;
    use windows_sys::Win32::UI::Shell::SHGetImageList;
    use windows_sys::Win32::UI::WindowsAndMessaging::HICON;
    const IID_IIMAGE_LIST: windows_sys::core::GUID =
        windows_sys::core::GUID::from_u128(0x46eb_5926_582e_4017_9fdf_e899_8daa_0950);
    const ILD_TRANSPARENT: u32 = 1;
    let mut pv: *mut c_void = std::ptr::null_mut();
    let hr = unsafe { SHGetImageList(shil, &IID_IIMAGE_LIST, &mut pv) };
    if hr < 0 || pv.is_null() {
        return None;
    }
    unsafe {
        let vtbl = *(pv as *mut *mut usize);
        let get_icon: unsafe extern "system" fn(*mut c_void, i32, u32, *mut HICON) -> i32 =
            std::mem::transmute(*vtbl.add(10));
        let mut h: HICON = std::ptr::null_mut();
        let ghr = get_icon(pv, index, ILD_TRANSPARENT, &mut h);
        release_unk(pv);
        if ghr < 0 || h.is_null() {
            None
        } else {
            Some(h)
        }
    }
}

#[cfg(windows)]
fn release_unk(p: *mut std::ffi::c_void) {
    if p.is_null() {
        return;
    }
    unsafe {
        let vtbl = *(p as *mut *mut usize);
        let rel: unsafe extern "system" fn(*mut std::ffi::c_void) -> u32 =
            std::mem::transmute(*vtbl.add(2));
        let _ = rel(p);
    }
}

#[cfg(windows)]
fn gdip_start() -> bool {
    use windows_sys::Win32::Graphics::GdiPlus::{GdiplusStartup, GdiplusStartupInput};
    let mut slot = GDIP_TOKEN.lock().unwrap_or_else(|e| e.into_inner());
    if slot.is_some() {
        return true;
    }
    let input = GdiplusStartupInput {
        GdiplusVersion: 1,
        DebugEventCallback: 0,
        SuppressBackgroundThread: 0,
        SuppressExternalCodecs: 0,
    };
    let mut token = 0usize;
    let st = unsafe { GdiplusStartup(&mut token, &input, std::ptr::null_mut()) };
    if st != 0 {
        return false;
    }
    *slot = Some(token);
    true
}

#[cfg(windows)]
fn png_clsid() -> Option<windows_sys::core::GUID> {
    use windows_sys::Win32::Graphics::GdiPlus::{
        GdipGetImageEncoders, GdipGetImageEncodersSize, ImageCodecInfo,
    };
    let mut num = 0u32;
    let mut bytes = 0u32;
    if unsafe { GdipGetImageEncodersSize(&mut num, &mut bytes) } != 0 || bytes == 0 {
        return None;
    }
    let mut buf = vec![0u8; bytes as usize];
    if unsafe { GdipGetImageEncoders(num, bytes, buf.as_mut_ptr() as *mut ImageCodecInfo) } != 0 {
        return None;
    }
    let info =
        unsafe { std::slice::from_raw_parts(buf.as_ptr() as *const ImageCodecInfo, num as usize) };
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
        if mime.eq_ignore_ascii_case("image/png") {
            return Some(enc.Clsid);
        }
    }
    None
}

#[cfg(windows)]
fn hicon_png_data_url(hicon: windows_sys::Win32::UI::WindowsAndMessaging::HICON) -> Option<String> {
    use windows_sys::Win32::Graphics::GdiPlus::{
        GdipCreateBitmapFromHICON, GdipDisposeImage, GdipSaveImageToStream, GpBitmap, GpImage,
    };
    use windows_sys::Win32::System::Com::StructuredStorage::{
        CreateStreamOnHGlobal, GetHGlobalFromStream,
    };
    use windows_sys::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};
    let clsid = png_clsid()?;
    let mut gp: *mut GpBitmap = std::ptr::null_mut();
    if unsafe { GdipCreateBitmapFromHICON(hicon, &mut gp) } != 0 || gp.is_null() {
        return None;
    }
    let mut stream: *mut std::ffi::c_void = std::ptr::null_mut();
    let hr = unsafe { CreateStreamOnHGlobal(std::ptr::null_mut(), 1, &mut stream) };
    if hr < 0 || stream.is_null() {
        unsafe {
            let _ = GdipDisposeImage(gp as *mut GpImage);
        }
        return None;
    }
    let st = unsafe { GdipSaveImageToStream(gp as *mut GpImage, stream, &clsid, std::ptr::null()) };
    unsafe {
        let _ = GdipDisposeImage(gp as *mut GpImage);
    }
    if st != 0 {
        release_unk(stream);
        return None;
    }
    let mut hg = std::ptr::null_mut();
    let hr = unsafe { GetHGlobalFromStream(stream, &mut hg) };
    if hr < 0 || hg.is_null() {
        release_unk(stream);
        return None;
    }
    let size = unsafe { GlobalSize(hg) };
    let ptr_mem = unsafe { GlobalLock(hg) };
    if ptr_mem.is_null() || size == 0 {
        unsafe {
            let _ = GlobalUnlock(hg);
        }
        release_unk(stream);
        return None;
    }
    let bytes = unsafe { std::slice::from_raw_parts(ptr_mem as *const u8, size) }.to_vec();
    unsafe {
        let _ = GlobalUnlock(hg);
    }
    release_unk(stream);
    Some(format!("data:image/png;base64,{}", b64(&bytes)))
}

#[cfg(windows)]
fn b64(data: &[u8]) -> String {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    let mut i = 0;
    while i < data.len() {
        let b0 = data[i] as u32;
        let b1 = if i + 1 < data.len() {
            data[i + 1] as u32
        } else {
            0
        };
        let b2 = if i + 2 < data.len() {
            data[i + 2] as u32
        } else {
            0
        };
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

#[cfg(test)]
mod tests {
    use super::{is_windows_icon, skips_app_logo};

    #[test]
    fn notepad_and_shell32_are_windows_icons() {
        assert!(is_windows_icon(r"C:\Windows\System32\notepad.exe"));
        assert!(is_windows_icon(r"C:\Windows\System32\shell32.dll"));
        assert!(is_windows_icon("notepad.exe"));
        assert!(is_windows_icon("shell32.dll"));
        assert!(is_windows_icon("imageres.dll,-102"));
        assert!(is_windows_icon(r"%SystemRoot%\System32\imageres.dll,-5"));
        assert!(is_windows_icon(r"%windir%\explorer.exe"));
    }

    #[test]
    fn chrome_and_msedge_program_files_are_not_windows_icons() {
        assert!(!is_windows_icon(
            r"C:\Program Files\Google\Chrome\Application\chrome.exe"
        ));
        assert!(!is_windows_icon(
            r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe"
        ));
    }

    #[test]
    fn inbox_photos_is_windows_icon_spotify_is_not() {
        assert!(is_windows_icon(
            r"C:\Program Files\WindowsApps\Microsoft.Windows.Photos_8wekyb3d8bbwe\Photos.exe"
        ));
        assert!(!is_windows_icon(
            r"C:\Program Files\WindowsApps\SpotifyAB.SpotifyMusic_1.0.0.0_x64__zpdnekdrzrea0\Spotify.exe"
        ));
    }

    #[test]
    fn txt_and_directories_never_get_logo() {
        assert!(skips_app_logo("dir", "Chrome"));
        assert!(skips_app_logo("dir", "notes.txt"));
        assert!(skips_app_logo("file", "notes.txt"));
        assert!(skips_app_logo("file", "NOTES.TXT"));
        assert!(!skips_app_logo("file", "Google Chrome.lnk"));
        assert!(!skips_app_logo("file", "chrome.exe"));
    }

    #[test]
    fn none_logo_is_omitted_from_json() {
        #[derive(serde::Serialize)]
        struct Row {
            name: &'static str,
            #[serde(skip_serializing_if = "Option::is_none")]
            logo: Option<String>,
        }
        let v = serde_json::to_value(&Row {
            name: "notes.txt",
            logo: None,
        })
        .unwrap();
        assert!(v.get("logo").is_none());
        assert_eq!(v["name"], "notes.txt");
    }
}
