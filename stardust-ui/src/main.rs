use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

const BIND: &str = "127.0.0.1:6006";
static DRIVE_SEQ: AtomicU64 = AtomicU64::new(1);
const HEADER_MAX: usize = 64 * 1024;
const BODY_MAX: usize = 8 * 1024 * 1024;
const STATIC_EXT: &[&str] = &["js", "css", "mjs", "map", "ico", "png", "svg", "woff2"];

struct HttpReq {
    method: String,
    target: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

fn read_http(stream: &mut TcpStream) -> io::Result<HttpReq> {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    loop {
        let n = stream.read(&mut tmp)?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&tmp[..n]);
        if let Some(pos) = find_header_end(&buf) {
            let header_bytes = &buf[..pos];
            let rest = &buf[pos + 4..];
            let headers_str = String::from_utf8_lossy(header_bytes);
            let mut lines = headers_str.split("\r\n");
            let req_line = lines.next().unwrap_or("");
            let mut parts = req_line.split_whitespace();
            let method = parts.next().unwrap_or("GET").to_string();
            let target = parts.next().unwrap_or("/").to_string();
            let mut headers = Vec::new();
            for line in lines {
                if let Some((k, v)) = line.split_once(':') {
                    headers.push((k.trim().to_string(), v.trim().to_string()));
                }
            }
            let content_length = headers
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case("Content-Length"))
                .and_then(|(_, v)| v.parse::<usize>().ok())
                .unwrap_or(0);
            if content_length > BODY_MAX {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "payload too large",
                ));
            }
            let mut body = rest.to_vec();
            while body.len() < content_length {
                let n = stream.read(&mut tmp)?;
                if n == 0 {
                    break;
                }
                body.extend_from_slice(&tmp[..n]);
            }
            body.truncate(content_length);
            return Ok(HttpReq {
                method,
                target,
                headers,
                body,
            });
        }
        if buf.len() > HEADER_MAX {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "headers too large",
            ));
        }
    }
    Err(io::Error::new(
        io::ErrorKind::UnexpectedEof,
        "incomplete request",
    ))
}

fn status_text(code: u16) -> &'static str {
    match code {
        200 => "OK",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        413 => "Payload Too Large",
        500 => "Internal Server Error",
        _ => "OK",
    }
}

fn reply(stream: &mut TcpStream, status: u16, content_type: &str, body: &[u8]) {
    let header = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len(),
        status = status,
        reason = status_text(status),
        content_type = content_type,
    );
    let _ = stream.write_all(header.as_bytes());
    let _ = stream.write_all(body);
    let _ = stream.flush();
}

fn reply_json(stream: &mut TcpStream, status: u16, value: &Value) {
    let body = serde_json::to_vec(value).unwrap_or_else(|_| b"{}".to_vec());
    reply(stream, status, "application/json; charset=utf-8", &body);
}

fn mime_of(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "html" | "htm" => "text/html; charset=utf-8",
        "js" | "mjs" => "application/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" | "map" => "application/json",
        "png" => "image/png",
        "svg" => "image/svg+xml",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        _ => "application/octet-stream",
    }
}

fn is_safe_rel(rel: &str) -> bool {
    let p = Path::new(rel);
    !rel.is_empty() && p.components().all(|c| matches!(c, Component::Normal(_)))
}

fn static_ext(route: &str) -> bool {
    Path::new(route)
        .extension()
        .and_then(|s| s.to_str())
        .map(|e| STATIC_EXT.iter().any(|x| x.eq_ignore_ascii_case(e)))
        .unwrap_or(false)
}

fn web_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("web")
}

fn serve_file(stream: &mut TcpStream, path: &Path) {
    match std::fs::read(path) {
        Ok(bytes) => reply(stream, 200, mime_of(path), &bytes),
        Err(_) => reply_json(stream, 404, &json!({ "error": "not found" })),
    }
}

fn serve_web(stream: &mut TcpStream, rel: &str) {
    if !is_safe_rel(rel) {
        reply_json(stream, 403, &json!({ "error": "invalid path" }));
        return;
    }
    serve_file(stream, &web_dir().join(rel));
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            let hex = |c: u8| match c {
                b'0'..=b'9' => Some(c - b'0'),
                b'a'..=b'f' => Some(c - b'a' + 10),
                b'A'..=b'F' => Some(c - b'A' + 10),
                _ => None,
            };
            if let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2])) {
                out.push((h << 4) | l);
                i += 3;
                continue;
            }
        }
        out.push(if b[i] == b'+' { b' ' } else { b[i] });
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn parse_query(q: &str) -> std::collections::HashMap<String, String> {
    let mut out = std::collections::HashMap::new();
    for part in q.split('&') {
        if part.is_empty() {
            continue;
        }
        let (k, v) = part.split_once('=').unwrap_or((part, ""));
        out.insert(percent_decode(k), percent_decode(v));
    }
    out
}

fn layout_path() -> PathBuf {
    match std::env::var_os("APPDATA") {
        Some(a) => PathBuf::from(a).join("StarDust").join("ui-layout.json"),
        None => PathBuf::from("stardust-ui-layout.json"),
    }
}

fn mode_file_path() -> PathBuf {
    match std::env::var_os("APPDATA") {
        Some(a) => PathBuf::from(a).join("StarDust").join("ui-mode.json"),
        None => PathBuf::from("ui-mode.json"),
    }
}

fn read_ui_mode_meta() -> (String, bool, bool) {
    let Ok(text) = std::fs::read_to_string(mode_file_path()) else {
        return ("windows".into(), false, false);
    };
    let v: Value = serde_json::from_str(&text).unwrap_or_else(|_| json!({}));
    let mode = match v.get("mode").and_then(|x| x.as_str()) {
        Some(s) if s.eq_ignore_ascii_case("stardust") => "stardust",
        _ => "windows",
    };
    let replace = v
        .get("replace_armed")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);
    let hide = v
        .get("hide_desktop_icons")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);
    (mode.to_string(), replace, hide)
}

fn sibling_exe(name: &str) -> PathBuf {
    match std::env::current_exe() {
        Ok(mut p) => {
            p.set_file_name(name);
            p
        }
        Err(_) => PathBuf::from(name),
    }
}

fn spawn_apply_mode(mode: &str, replace: bool, hide: bool) -> Result<(), String> {
    let open = sibling_exe("stardust-open.exe");
    if !open.is_file() {
        return Err("stardust-open.exe missing".into());
    }
    let out = std::env::temp_dir().join(format!(
        "stardust-ui-mode-{}.json",
        std::process::id()
    ));
    let mut cmd = Command::new(&open);
    cmd.arg("--apply-mode").arg(mode);
    if replace {
        cmd.arg("--replace");
    }
    if hide {
        cmd.arg("--hide-icons");
    }
    cmd.env("STARDUST_UI_MODE_OUT", &out);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // DETACHED_PROCESS | CREATE_NO_WINDOW — restyle after this HTTP reply.
        cmd.creation_flags(0x08000008);
    }
    cmd.spawn().map(|_| ()).map_err(|e| e.to_string())
}

fn handle_ui_mode(stream: &mut TcpStream, v: &Value) {
    let mode_s = v.get("mode").and_then(|x| x.as_str()).unwrap_or("");
    let mode = if mode_s.eq_ignore_ascii_case("stardust") {
        "stardust"
    } else if mode_s.eq_ignore_ascii_case("windows") {
        "windows"
    } else {
        reply_json(
            stream,
            400,
            &json!({ "error": "mode must be stardust or windows" }),
        );
        return;
    };
    let replace = v.get("replace").and_then(|x| x.as_bool()).unwrap_or(false);
    let hide = v
        .get("hideDesktopIcons")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);
    if sibling_exe("stardust-open.exe").is_file() == false {
        reply_json(
            stream,
            500,
            &json!({ "error": "stardust-open.exe missing" }),
        );
        return;
    }
    let replace_armed = replace && mode == "stardust";
    let hide_icons = hide && mode == "stardust";
    // Reply first: apply_mode restyles StarDustHost and aborts an in-flight fetch.
    reply_json(
        stream,
        200,
        &json!({
            "ok": true,
            "mode": mode,
            "replaceArmed": replace_armed,
            "hideDesktopIcons": hide_icons,
        }),
    );
    if let Err(_) = spawn_apply_mode(mode, replace, hide) {
        // Mode JSON is unchanged; the next /api/meta refresh shows the real mode.
    }
}

fn drive_ipc_dir() -> PathBuf {
    std::env::temp_dir().join("stardust-drive")
}

fn drive_helper_alive() -> bool {
    let beat = drive_ipc_dir().join("heartbeat");
    let Ok(text) = std::fs::read_to_string(&beat) else {
        return false;
    };
    let Ok(ts) = text.trim().parse::<u64>() else {
        return false;
    };
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    now.saturating_sub(ts) < 3
}

fn ensure_drive_helper() -> Result<(), String> {
    if drive_helper_alive() {
        return Ok(());
    }
    let exe = sibling_exe("stardust-drive.exe");
    if !exe.is_file() {
        return Err("stardust-drive.exe missing".into());
    }
    let mut cmd = Command::new(&exe);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000008);
    }
    cmd.spawn().map_err(|e| e.to_string())?;
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        if drive_helper_alive() {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(20));
    }
    if drive_helper_alive() {
        Ok(())
    } else {
        Err("drive helper did not start".into())
    }
}

fn handle_drive(stream: &mut TcpStream, v: &Value) {
    if let Err(e) = ensure_drive_helper() {
        reply_json(stream, 500, &json!({ "error": e }));
        return;
    }
    let dir = drive_ipc_dir();
    let _ = std::fs::create_dir_all(&dir);
    let id = DRIVE_SEQ.fetch_add(1, Ordering::Relaxed);
    let req = dir.join(format!("req-{id}.json"));
    let resp = dir.join(format!("resp-{id}.json"));
    let _ = std::fs::remove_file(&resp);
    if let Err(e) = std::fs::write(&req, serde_json::to_vec(v).unwrap_or_default()) {
        reply_json(stream, 500, &json!({ "error": e.to_string() }));
        return;
    }
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        if let Ok(text) = std::fs::read_to_string(&resp) {
            let _ = std::fs::remove_file(&resp);
            match serde_json::from_str::<Value>(&text) {
                Ok(wrap) => {
                    let status = wrap.get("status").and_then(|x| x.as_u64()).unwrap_or(200) as u16;
                    let body = wrap.get("body").cloned().unwrap_or(json!({}));
                    reply_json(stream, status, &body);
                }
                Err(_) => reply_json(stream, 500, &json!({ "error": "bad drive response" })),
            }
            return;
        }
        if Instant::now() >= deadline {
            let _ = std::fs::remove_file(&req);
            reply_json(stream, 500, &json!({ "error": "drive helper timed out" }));
            return;
        }
        thread::sleep(Duration::from_millis(5));
    }
}

fn list_drives() -> Value {
    let mut drives = Vec::new();
    for c in b'A'..=b'Z' {
        let p = format!("{}:\\", c as char);
        if Path::new(&p).is_dir() {
            drives.push(json!({
                "name": format!("{}:", c as char),
                "path": p,
                "kind": "dir",
            }));
        }
    }
    json!({ "drives": drives })
}

fn handle_open_cursor(stream: &mut TcpStream) {
    let open = sibling_exe("stardust-open.exe");
    if !open.is_file() {
        reply_json(
            stream,
            500,
            &json!({ "error": "stardust-open.exe missing" }),
        );
        return;
    }
    let out = std::env::temp_dir().join(format!(
        "stardust-open-cursor-{}.json",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&out);
    let mut cmd = Command::new(&open);
    cmd.arg("--open-cursor");
    cmd.env("STARDUST_OPEN_CURSOR_OUT", &out);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            reply_json(stream, 500, &json!({ "error": e.to_string() }));
            return;
        }
    };
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Ok(text) = std::fs::read_to_string(&out) {
            let _ = std::fs::remove_file(&out);
            let _ = child.try_wait();
            match serde_json::from_str::<Value>(&text) {
                Ok(body) if body.get("ok").and_then(|x| x.as_bool()) == Some(true) => {
                    reply_json(stream, 200, &json!({ "ok": true }));
                }
                Ok(body) => {
                    let status = body.get("status").and_then(|x| x.as_u64()).unwrap_or(500) as u16;
                    let err = body
                        .get("error")
                        .cloned()
                        .unwrap_or_else(|| json!("open Cursor failed"));
                    reply_json(stream, status.max(400), &json!({ "error": err }));
                }
                Err(_) => reply_json(stream, 500, &json!({ "error": "bad open-cursor response" })),
            }
            return;
        }
        match child.try_wait() {
            Ok(Some(_)) => {
                thread::sleep(Duration::from_millis(30));
                if std::fs::read_to_string(&out).is_err() {
                    reply_json(stream, 500, &json!({ "error": "open-cursor helper exited" }));
                    return;
                }
            }
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(20)),
            Ok(None) => {
                let _ = child.kill();
                reply_json(stream, 500, &json!({ "error": "open Cursor timed out" }));
                return;
            }
            Err(e) => {
                reply_json(stream, 500, &json!({ "error": e.to_string() }));
                return;
            }
        }
    }
}

fn handle_rebuild_ui(stream: &mut TcpStream, v: &Value) {
    let open = sibling_exe("stardust-open.exe");
    if !open.is_file() {
        reply_json(
            stream,
            500,
            &json!({ "error": "stardust-open.exe missing" }),
        );
        return;
    }
    let dir = std::env::temp_dir();
    let id = std::process::id();
    let req = dir.join(format!("stardust-rebuild-in-{id}.json"));
    let out = dir.join(format!("stardust-rebuild-out-{id}.json"));
    let _ = std::fs::remove_file(&out);
    if let Err(e) = std::fs::write(&req, serde_json::to_vec(v).unwrap_or_else(|_| b"{}".to_vec())) {
        reply_json(stream, 500, &json!({ "error": e.to_string() }));
        return;
    }
    let mut cmd = Command::new(&open);
    cmd.arg("--rebuild-ui");
    cmd.env("STARDUST_REBUILD_IN", &req);
    cmd.env("STARDUST_REBUILD_OUT", &out);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            let _ = std::fs::remove_file(&req);
            reply_json(stream, 500, &json!({ "error": e.to_string() }));
            return;
        }
    };
    let deadline = Instant::now() + Duration::from_secs(600);
    loop {
        if let Ok(text) = std::fs::read_to_string(&out) {
            let _ = std::fs::remove_file(&out);
            let _ = std::fs::remove_file(&req);
            let _ = child.try_wait();
            match serde_json::from_str::<Value>(&text) {
                Ok(body) => {
                    let ok = body.get("ok").and_then(|x| x.as_bool()) != Some(false);
                    let relaunch = body.get("relaunch").and_then(|x| x.as_bool()) == Some(true);
                    if ok {
                        reply_json(stream, 200, &body);
                        if relaunch {
                            thread::spawn(|| {
                                thread::sleep(Duration::from_millis(400));
                                std::process::exit(0);
                            });
                        }
                    } else {
                        let status = body.get("status").and_then(|x| x.as_u64()).unwrap_or(500) as u16;
                        reply_json(stream, status.max(400), &body);
                    }
                }
                Err(_) => reply_json(stream, 500, &json!({ "error": "bad rebuild response" })),
            }
            return;
        }
        match child.try_wait() {
            Ok(Some(_)) => {
                thread::sleep(Duration::from_millis(50));
                if std::fs::read_to_string(&out).is_err() {
                    let _ = std::fs::remove_file(&req);
                    reply_json(stream, 500, &json!({ "error": "rebuild helper exited" }));
                    return;
                }
            }
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(200)),
            Ok(None) => {
                let _ = child.kill();
                let _ = std::fs::remove_file(&req);
                reply_json(stream, 504, &json!({ "error": "cargo build timed out" }));
                return;
            }
            Err(e) => {
                let _ = std::fs::remove_file(&req);
                reply_json(stream, 500, &json!({ "error": e.to_string() }));
                return;
            }
        }
    }
}

fn json_path(v: &Value) -> Option<String> {
    v.get("path").and_then(|x| x.as_str()).map(|s| s.to_string())
}

fn forbidden_open_path(p: &Path) -> bool {
    p.components().any(|c| {
        let n = c.as_os_str().to_string_lossy();
        n.eq_ignore_ascii_case("AppData")
            || n.eq_ignore_ascii_case("Application Data")
            || n.to_ascii_lowercase().starts_with("stardust-ws-")
    })
}

fn spawn_cmd_no_window(cmd: &mut Command) -> Result<(), String> {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    cmd.spawn().map(|_| ()).map_err(|e| e.to_string())
}

fn handle_open_external(stream: &mut TcpStream, v: &Value) {
    let Some(raw) = json_path(v) else {
        reply_json(stream, 400, &json!({ "error": "missing path" }));
        return;
    };
    let p = PathBuf::from(&raw);
    if !p.is_absolute() {
        reply_json(stream, 400, &json!({ "error": "path must be absolute" }));
        return;
    }
    if forbidden_open_path(&p) {
        reply_json(
            stream,
            400,
            &json!({ "error": "refuses AppData / agent temp workspace" }),
        );
        return;
    }
    if !p.exists() {
        reply_json(stream, 404, &json!({ "error": "not found" }));
        return;
    }
    if p.is_dir() {
        reply_json(
            stream,
            400,
            &json!({ "error": "path is a directory; use jump" }),
        );
        return;
    }
    let path_s = p.to_string_lossy().into_owned();
    let mut cmd = Command::new("cmd");
    cmd.args(["/C", "start", "", &path_s]);
    match spawn_cmd_no_window(&mut cmd) {
        Ok(()) => reply_json(stream, 200, &json!({ "ok": true })),
        Err(e) => reply_json(stream, 500, &json!({ "error": e })),
    }
}

fn handle_open_explorer(stream: &mut TcpStream, v: &Value) {
    let Some(raw) = json_path(v) else {
        reply_json(stream, 400, &json!({ "error": "missing path" }));
        return;
    };
    let p = PathBuf::from(&raw);
    if !p.exists() {
        reply_json(stream, 404, &json!({ "error": "not found" }));
        return;
    }
    let sys = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    let mut cmd = Command::new(PathBuf::from(sys).join("explorer.exe"));
    cmd.arg(format!("/e,{}", p.display()));
    match spawn_cmd_no_window(&mut cmd) {
        Ok(()) => reply_json(stream, 200, &json!({ "ok": true })),
        Err(e) => reply_json(stream, 500, &json!({ "error": e })),
    }
}

fn spawn_view_desktop() {
    let view = sibling_exe("stardust-view.exe");
    if !view.is_file() {
        return;
    }
    let mut cmd = Command::new(&view);
    cmd.arg("--desktop");
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000008);
    }
    let _ = cmd.spawn();
}

fn read_layout() -> Value {
    let Ok(text) = std::fs::read_to_string(layout_path()) else {
        return json!({});
    };
    serde_json::from_str(&text).unwrap_or_else(|_| json!({}))
}

fn desktop_dir() -> PathBuf {
    if let Ok(home) = std::env::var("USERPROFILE") {
        let od = PathBuf::from(&home).join("OneDrive").join("Desktop");
        if od.is_dir() {
            return od;
        }
        return PathBuf::from(home).join("Desktop");
    }
    PathBuf::from(".")
}

fn public_desktop_dir() -> Option<PathBuf> {
    let d = PathBuf::from(std::env::var_os("PUBLIC")?).join("Desktop");
    if d.is_dir() {
        Some(d)
    } else {
        None
    }
}

fn path_key(p: &Path) -> String {
    p.to_string_lossy()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_ascii_lowercase()
}

fn same_dir(a: &Path, b: &Path) -> bool {
    path_key(a) == path_key(b)
}

fn is_user_desktop(path: &Path) -> bool {
    same_dir(path, &desktop_dir())
}

fn skips_app_logo(kind: &str, name: &str) -> bool {
    kind != "file" || name.to_ascii_lowercase().ends_with(".txt")
}

fn is_windows_icon(icon_source: &str) -> bool {
    let n = icon_source.replace('/', "\\").to_ascii_lowercase();
    let file = n.rsplit('\\').next().unwrap_or(&n);
    let file = match file.find(',') {
        Some(i) if i > 0 => &file[..i],
        _ => file,
    };
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
    if SHELL_MODULES.iter().any(|m| file.eq_ignore_ascii_case(m)) {
        return true;
    }
    if n.contains(r"\systemapps\") {
        return true;
    }
    if let Some(i) = n.find(r"\windowsapps\") {
        let rest = &n[i + r"\windowsapps\".len()..];
        let pkg = rest.split('\\').next().unwrap_or("");
        if pkg.starts_with("microsoft.windows.")
            || pkg.starts_with("microsoft.microsoftedge")
            || pkg.starts_with("microsoft.edge.")
        {
            return true;
        }
    }
    let mut parts = n.trim_start_matches('\\').split('\\').filter(|s| !s.is_empty());
    let Some(first) = parts.next() else {
        return false;
    };
    let win = if first.len() == 2 && first.as_bytes().get(1) == Some(&b':') {
        parts.next().unwrap_or("")
    } else {
        first
    };
    win == "windows"
}

static LOGO_CACHE: Mutex<Option<HashMap<(String, u64), Option<String>>>> = Mutex::new(None);

fn logo_cache_get(path: &Path, mtime: u64) -> Option<Option<String>> {
    let key = (path_key(path), mtime);
    LOGO_CACHE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get_or_insert_with(HashMap::new)
        .get(&key)
        .cloned()
}

fn logo_cache_put(path: &Path, mtime: u64, val: Option<String>) {
    let key = (path_key(path), mtime);
    LOGO_CACHE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get_or_insert_with(HashMap::new)
        .insert(key, val);
}

fn collect_dir_entries(dir: &Path, include_abs_path: bool) -> Vec<Value> {
    let mut entries = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir) else {
        return entries;
    };
    for ent in rd.flatten() {
        let name = ent.file_name().to_string_lossy().into_owned();
        if name.eq_ignore_ascii_case("desktop.ini") {
                continue;
            }
        let child = ent.path();
        let meta = ent.metadata().ok();
        let is_dir = meta.as_ref().map(|m| m.is_dir()).unwrap_or(child.is_dir());
        let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
        let mtime = meta
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let mut row = json!({
            "name": name,
            "kind": if is_dir { "dir" } else { "file" },
            "size": size,
            "mtime": mtime,
            "isText": false,
        });
        if include_abs_path {
            if let Some(obj) = row.as_object_mut() {
                obj.insert("path".into(), json!(child.to_string_lossy()));
            }
        }
        entries.push(row);
    }
    entries
}

fn overlay_public_desktop(listed: &Path, entries: &mut Vec<Value>) {
    if !is_user_desktop(listed) {
        return;
    }
    let Some(public) = public_desktop_dir() else {
        return;
    };
    if same_dir(listed, &public) {
        return;
    }
    let mut have = std::collections::HashSet::new();
    for e in entries.iter() {
        if let Some(n) = e.get("name").and_then(|x| x.as_str()) {
            have.insert(n.to_ascii_lowercase());
        }
    }
    for extra in collect_dir_entries(&public, true) {
        let Some(n) = extra.get("name").and_then(|x| x.as_str()) else {
            continue;
        };
        if have.contains(&n.to_ascii_lowercase()) {
            continue;
        }
        have.insert(n.to_ascii_lowercase());
        entries.push(extra);
    }
}

fn extract_logos_powershell(jobs: &[(usize, PathBuf, u64)]) -> HashMap<usize, String> {
    let mut out = HashMap::new();
    if jobs.is_empty() {
        return out;
    }
    let script = r#"
Add-Type -AssemblyName System.Drawing | Out-Null
$sh = New-Object -ComObject WScript.Shell
foreach ($line in [Console]::In.ReadToEnd().Split("`n")) {
  $line = $line.Trim()
  if (-not $line) { continue }
  $parts = $line.Split("`t", 2)
  if ($parts.Length -lt 2) { continue }
  $i = $parts[0]
  $p = $parts[1]
  $src = $p
  if ($p -like '*.lnk') {
    try {
      $t = $sh.CreateShortcut($p).TargetPath
      if ($t) { $src = $t }
    } catch {}
  }
  $b64 = $null
  foreach ($c in @($p, $src)) {
    if (-not $c) { continue }
    try {
      $ico = [System.Drawing.Icon]::ExtractAssociatedIcon($c)
      if (-not $ico) { continue }
      $bmp = $ico.ToBitmap()
      $ms = New-Object System.IO.MemoryStream
      $bmp.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
      $b64 = [Convert]::ToBase64String($ms.ToArray())
      $ms.Dispose(); $bmp.Dispose(); $ico.Dispose()
      if ($b64) { break }
    } catch {}
  }
  if ($b64) { Write-Output ("{0}`t{1}`t{2}" -f $i, $src, $b64) }
}
"#;
    let mut cmd = Command::new("powershell");
    cmd.args(["-NoProfile", "-STA", "-Command", script])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    let Ok(mut child) = cmd.spawn() else {
        return out;
    };
    if let Some(mut stdin) = child.stdin.take() {
        let mut payload = String::new();
        for (i, path, _) in jobs {
            payload.push_str(&format!("{i}\t{}\n", path.to_string_lossy()));
        }
        let _ = stdin.write_all(payload.as_bytes());
    }
    let Ok(piped) = child.wait_with_output() else {
        return out;
    };
    if !piped.status.success() {
        return out;
    }
    for line in String::from_utf8_lossy(&piped.stdout).lines() {
        let mut bits = line.splitn(3, '\t');
        let Some(i_s) = bits.next() else { continue };
        let Some(src) = bits.next() else { continue };
        let Some(b64) = bits.next() else { continue };
        if is_windows_icon(src) {
            continue;
        }
        let Ok(i) = i_s.parse::<usize>() else { continue };
        if b64.is_empty() {
            continue;
        }
        out.insert(i, format!("data:image/png;base64,{b64}"));
    }
    out
}

fn fill_app_logos(listed: &Path, entries: &mut [Value]) {
    let mut jobs = Vec::new();
    let mut cached = Vec::new();
    for (i, e) in entries.iter().enumerate() {
        let kind = e.get("kind").and_then(|x| x.as_str()).unwrap_or("");
        let name = e.get("name").and_then(|x| x.as_str()).unwrap_or("");
        if skips_app_logo(kind, name) {
            continue;
        }
        let path = e
            .get("path")
            .and_then(|x| x.as_str())
            .map(PathBuf::from)
            .unwrap_or_else(|| listed.join(name));
        let mtime = e.get("mtime").and_then(|x| x.as_u64()).unwrap_or(0);
        if let Some(hit) = logo_cache_get(&path, mtime) {
            if let Some(url) = hit {
                cached.push((i, url));
            }
            continue;
        }
        jobs.push((i, path, mtime));
    }
    for (i, url) in cached {
        if let Some(obj) = entries[i].as_object_mut() {
            obj.insert("logo".into(), json!(url));
        }
    }
    let got = extract_logos_powershell(&jobs);
    for (i, path, mtime) in jobs {
        let url = got.get(&i).cloned();
        logo_cache_put(&path, mtime, url.clone());
        if let Some(url) = url {
            if let Some(obj) = entries[i].as_object_mut() {
                obj.insert("logo".into(), json!(url));
            }
        }
    }
}

fn list_children(path: &str) -> Value {
    let p = if path.is_empty() {
        desktop_dir()
    } else {
        PathBuf::from(path)
    };
    let mut entries = collect_dir_entries(&p, false);
    overlay_public_desktop(&p, &mut entries);
    fill_app_logos(&p, &mut entries);
    json!({
        "path": p.to_string_lossy(),
        "entries": entries,
    })
}

fn handle_http(mut stream: TcpStream) {
    let _ = stream.set_nodelay(true);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(30)));
    let req = match read_http(&mut stream) {
        Ok(r) => r,
        Err(e) if e.to_string().contains("payload too large") => {
            reply_json(&mut stream, 413, &json!({ "error": "payload too large" }));
            return;
        }
        Err(_) => return,
    };
    let (route, query) = match req.target.split_once('?') {
        Some((p, q)) => (p.to_string(), q.to_string()),
        None => (req.target.clone(), String::new()),
    };
    if route == "/api/healthz" && req.method == "GET" {
        reply_json(&mut stream, 200, &json!({ "ok": true }));
        return;
    }
    let q = parse_query(&query);
    match (req.method.as_str(), route.as_str()) {
        ("GET", "/") => serve_file(&mut stream, &web_dir().join("index.html")),
        ("GET", "/api/meta") => {
            let desktop = desktop_dir().to_string_lossy().into_owned();
            let (ui_mode, replace_armed, hide_icons) = read_ui_mode_meta();
            reply_json(
                &mut stream,
                200,
                &json!({
                    "defaultRoot": desktop,
                    "sessionRoot": desktop,
                    "desktopPath": desktop,
                    "uiMode": ui_mode,
                    "replaceArmed": replace_armed,
                    "hideDesktopIcons": hide_icons,
                    "hasCursorKey": false,
                }),
            );
        }
        ("POST", "/api/ui-mode") => match serde_json::from_slice::<Value>(&req.body) {
            Ok(v) if v.is_object() => handle_ui_mode(&mut stream, &v),
            _ => reply_json(&mut stream, 400, &json!({ "error": "missing body" })),
        },
        ("POST", "/api/drive") => match serde_json::from_slice::<Value>(&req.body) {
            Ok(v) if v.is_object() => handle_drive(&mut stream, &v),
            _ => reply_json(&mut stream, 400, &json!({ "error": "missing body" })),
        },
        ("POST", "/api/open-cursor") => handle_open_cursor(&mut stream),
        ("POST", "/api/open-external") => match serde_json::from_slice::<Value>(&req.body) {
            Ok(v) if v.is_object() => handle_open_external(&mut stream, &v),
            _ => reply_json(&mut stream, 400, &json!({ "error": "missing body" })),
        },
        ("POST", "/api/open-explorer") => match serde_json::from_slice::<Value>(&req.body) {
            Ok(v) if v.is_object() => handle_open_explorer(&mut stream, &v),
            _ => reply_json(&mut stream, 400, &json!({ "error": "missing body" })),
        },
        ("POST", "/api/rebuild-ui") => match serde_json::from_slice::<Value>(&req.body) {
            Ok(v) if v.is_object() => handle_rebuild_ui(&mut stream, &v),
            Ok(_) => handle_rebuild_ui(&mut stream, &json!({})),
            Err(_) if req.body.is_empty() => handle_rebuild_ui(&mut stream, &json!({})),
            Err(_) => reply_json(&mut stream, 400, &json!({ "error": "missing body" })),
        },
        ("GET", "/api/drives") => reply_json(&mut stream, 200, &list_drives()),
        ("GET", "/api/children") => {
            let path = q.get("path").map(String::as_str).unwrap_or("");
            reply_json(&mut stream, 200, &list_children(path));
        }
        ("GET", "/api/file") => {
            let path = q.get("path").map(String::as_str).unwrap_or("");
            match std::fs::read_to_string(path) {
                Ok(content) => reply_json(&mut stream, 200, &json!({ "content": content })),
                Err(_) => reply_json(&mut stream, 404, &json!({ "error": "not found" })),
            }
        }
        ("GET", "/api/exists") => {
            let path = q.get("path").map(String::as_str).unwrap_or("");
            reply_json(
                &mut stream,
                200,
                &json!({ "exists": Path::new(path).exists() }),
            );
        }
        ("GET", "/api/stat") => {
            let path = q.get("path").map(String::as_str).unwrap_or("");
            let p = Path::new(path);
            let is_dir = p.is_dir();
            reply_json(
                        &mut stream,
                        200,
                        &json!({
                    "path": path,
                    "kind": if is_dir { "dir" } else { "file" },
                    "exists": p.exists(),
                }),
            );
        }
        ("GET", "/api/layout") => reply_json(&mut stream, 200, &read_layout()),
        ("PUT", "/api/layout") => {
            if req.body.is_empty() {
                reply_json(&mut stream, 400, &json!({ "error": "missing body" }));
                        } else {
                match serde_json::from_slice::<Value>(&req.body) {
                    Ok(v) if v.is_object() => {
                        let p = layout_path();
                        if let Some(parent) = p.parent() {
                            let _ = std::fs::create_dir_all(parent);
                        }
                        match serde_json::to_vec_pretty(&v) {
                            Ok(bytes) => match std::fs::write(&p, bytes) {
                                Ok(()) => reply_json(&mut stream, 200, &json!({ "ok": true })),
                                Err(e) => {
                                    reply_json(&mut stream, 500, &json!({ "error": e.to_string() }))
                                }
                            },
                            Err(e) => {
                                reply_json(&mut stream, 400, &json!({ "error": e.to_string() }))
                            }
                        }
                    }
                    _ => reply_json(
                        &mut stream,
                        400,
                        &json!({ "error": "layout body must be a JSON object" }),
                    ),
                }
            }
        }
        ("GET", route) if route.starts_with("/web/") => {
            serve_web(&mut stream, route.trim_start_matches("/web/"));
        }
        ("GET", route) if static_ext(route) => {
            serve_web(&mut stream, route.trim_start_matches('/'));
        }
        _ => reply_json(&mut stream, 404, &json!({ "error": "not found" })),
    }
}

fn spawn_server() -> bool {
    match TcpListener::bind(BIND) {
        Ok(listener) => {
            thread::spawn(move || {
                for stream in listener.incoming() {
                    if let Ok(stream) = stream {
                        thread::spawn(move || handle_http(stream));
                    }
                }
            });
            true
        }
        Err(_) => false,
    }
}

fn main() {
    let desktop = std::env::args().any(|a| a == "--desktop");
    if !spawn_server() {
        std::process::exit(1);
    }
    if desktop {
        spawn_view_desktop();
    }
    loop {
        thread::sleep(Duration::from_secs(60));
    }
}
