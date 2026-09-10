mod cns;
mod host;
#[cfg(windows)]
mod desktop_drive;

use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Component, Path};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::RecvTimeoutError;
use std::thread;
use std::time::Duration;

use serde_json::{json, Value};

const BIND: &str = "127.0.0.1:6006";
const AGENT: &str = "127.0.0.1:6007";
static SIDECAR: std::sync::Mutex<Option<Child>> = std::sync::Mutex::new(None);
const HEADER_MAX: usize = 64 * 1024;
const BODY_MAX: usize = 8 * 1024 * 1024;
const STATIC_EXT: &[&str] = &["js", "css", "mjs", "map", "ico", "png", "svg", "woff2"];

struct HttpReq {
    method: String,
    target: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

fn header<'a>(req: &'a HttpReq, name: &str) -> Option<&'a str> {
    req.headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
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
        405 => "Method Not Allowed",
        409 => "Conflict",
        413 => "Payload Too Large",
        415 => "Unsupported Media Type",
        500 => "Internal Server Error",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
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

fn reply_api(stream: &mut TcpStream, r: host::ApiResult) {
    match r {
        Ok(v) => reply_json(stream, 200, &v),
        Err(e) => reply_json(stream, e.status, &e.body),
    }
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
    !rel.is_empty()
        && p.components()
            .all(|c| matches!(c, Component::Normal(_)))
}

fn static_ext(route: &str) -> bool {
    Path::new(route)
        .extension()
        .and_then(|s| s.to_str())
        .map(|e| STATIC_EXT.iter().any(|x| x.eq_ignore_ascii_case(e)))
        .unwrap_or(false)
}

fn serve_file(stream: &mut TcpStream, path: &Path) {
    match fs_read(path) {
        Ok(bytes) => reply(stream, 200, mime_of(path), &bytes),
        Err(_) => reply_json(stream, 404, &json!({ "error": "not found" })),
    }
}

fn fs_read(path: &Path) -> io::Result<Vec<u8>> {
    std::fs::read(path)
}

fn serve_web(stream: &mut TcpStream, rel: &str) {
    if !is_safe_rel(rel) {
        reply_json(stream, 403, &json!({ "error": "invalid path" }));
        return;
    }
    let path = host::web_dir().join(rel);
    serve_file(stream, &path);
}

fn body_json(req: &HttpReq) -> Result<Value, host::ApiError> {
    if req.body.is_empty() {
        return Err(host::ApiError::bad("missing body"));
    }
    serde_json::from_slice(&req.body).map_err(|e| host::ApiError::bad(e.to_string()))
}

fn json_str<'a>(v: &'a Value, key: &str) -> Result<&'a str, host::ApiError> {
    v.get(key)
        .and_then(|x| x.as_str())
        .ok_or_else(|| host::ApiError::bad(format!("missing {key}")))
}

fn sse_open(stream: &mut TcpStream) -> bool {
    let headers = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: keep-alive\r\n\r\n";
    if stream.write_all(headers.as_bytes()).is_err() {
        return false;
    }
    stream.flush().is_ok()
}

fn handle_watch(stream: &mut TcpStream, path: &str) {
    if !sse_open(stream) {
        return;
    }
    let started = match host::start_watch(path) {
        Ok(x) => x,
        Err(_) => {
            loop {
                if stream.write_all(b": keepalive\n\n").is_err() {
                    return;
                }
                if stream.flush().is_err() {
                    return;
                }
                thread::sleep(Duration::from_secs(15));
            }
        }
    };
    let (watcher, rx, watch_path) = started;
    loop {
        match rx.recv_timeout(Duration::from_secs(15)) {
            Ok(Ok(event)) => {
                let kind = host::event_kind_str(&event.kind);
                let paths = if event.paths.is_empty() {
                    vec![watch_path.clone()]
                } else {
                    event.paths
                };
                for p in paths {
                    let data = json!({
                        "path": host::path_to_string(&p),
                        "kind": kind,
                    });
                    let line = format!("data: {data}\n\n");
                    if stream.write_all(line.as_bytes()).is_err() {
                        drop(watcher);
                        return;
                    }
                    let _ = stream.flush();
                }
            }
            Ok(Err(_)) => {
                let data = json!({
                    "path": host::path_to_string(&watch_path),
                    "kind": "other",
                });
                let line = format!("data: {data}\n\n");
                if stream.write_all(line.as_bytes()).is_err() {
                    drop(watcher);
                    return;
                }
                let _ = stream.flush();
            }
            Err(RecvTimeoutError::Timeout) => {
                if stream.write_all(b": keepalive\n\n").is_err() {
                    drop(watcher);
                    return;
                }
                let _ = stream.flush();
            }
            Err(RecvTimeoutError::Disconnected) => {
                drop(watcher);
                return;
            }
        }
    }
}

fn agent_upstream(route: &str, query: &str) -> String {
    let path = if route == "/api/agent" {
        "/"
    } else if let Some(rest) = route.strip_prefix("/api/agent") {
        rest
    } else {
        route
    };
    if query.is_empty() {
        path.to_string()
    } else {
        format!("{path}?{query}")
    }
}

fn sidecar_unavailable() -> Value {
    json!({ "error": "sidecar unavailable" })
}

fn agent_health(stream: &mut TcpStream) {
    let addr: std::net::SocketAddr = match AGENT.parse() {
        Ok(a) => a,
        Err(_) => {
            reply_json(stream, 200, &json!({ "ok": false }));
            return;
        }
    };
    let mut up = match TcpStream::connect_timeout(&addr, Duration::from_millis(400)) {
        Ok(s) => s,
        Err(_) => {
            spawn_sidecar();
            match TcpStream::connect_timeout(&addr, Duration::from_millis(400)) {
                Ok(s) => s,
                Err(_) => {
                    reply_json(stream, 200, &json!({ "ok": false }));
                    return;
                }
            }
        }
    };
    let _ = up.set_read_timeout(Some(Duration::from_millis(400)));
    let _ = up.set_write_timeout(Some(Duration::from_millis(400)));
    let req = b"GET /health HTTP/1.1\r\nHost: 127.0.0.1:6007\r\nConnection: close\r\n\r\n";
    if up.write_all(req).is_err() {
        reply_json(stream, 200, &json!({ "ok": false }));
        return;
    }
    let mut resp = Vec::new();
    if up.read_to_end(&mut resp).is_err() || resp.is_empty() {
        reply_json(stream, 200, &json!({ "ok": false }));
        return;
    }
    let text = String::from_utf8_lossy(&resp);
    let body = text.split("\r\n\r\n").nth(1).unwrap_or("").trim();
    match serde_json::from_str::<Value>(body) {
        Ok(v) => reply_json(stream, 200, &v),
        Err(_) => reply_json(stream, 200, &json!({ "ok": false })),
    }
}

fn sidecar_listening() -> bool {
    let Ok(addr) = AGENT.parse() else {
        return false;
    };
    TcpStream::connect_timeout(&addr, Duration::from_millis(150)).is_ok()
}

fn notify_sidecar_workspace(body: &Value) {
    let addr: std::net::SocketAddr = match AGENT.parse() {
        Ok(a) => a,
        Err(_) => return,
    };
    let mut up = match TcpStream::connect_timeout(&addr, Duration::from_millis(400)) {
        Ok(s) => s,
        Err(_) => return,
    };
    let payload = json!({
        "cwd": body.get("cwd"),
        "map": body.get("map"),
    })
    .to_string();
    let req = format!(
        "POST /workspace HTTP/1.1\r\nHost: 127.0.0.1:6007\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        payload.len(),
        payload
    );
    let _ = up.set_read_timeout(Some(Duration::from_millis(400)));
    let _ = up.set_write_timeout(Some(Duration::from_millis(400)));
    if up.write_all(req.as_bytes()).is_err() {
        return;
    }
    let mut sink = Vec::new();
    let _ = up.read_to_end(&mut sink);
}

fn sidecar_reported_mtime_secs() -> Option<u64> {
    let addr = AGENT.parse().ok()?;
    let mut up = TcpStream::connect_timeout(&addr, Duration::from_millis(300)).ok()?;
    let _ = up.set_read_timeout(Some(Duration::from_millis(300)));
    let _ = up.set_write_timeout(Some(Duration::from_millis(300)));
    let req = b"GET /health HTTP/1.1\r\nHost: 127.0.0.1:6007\r\nConnection: close\r\n\r\n";
    up.write_all(req).ok()?;
    let mut resp = Vec::new();
    up.read_to_end(&mut resp).ok()?;
    let text = String::from_utf8_lossy(&resp);
    let body = text.split("\r\n\r\n").nth(1).unwrap_or("").trim();
    let json_start = body.find('{')?;
    let v: Value = serde_json::from_str(&body[json_start..]).ok()?;
    v.get("scriptMtime")
        .and_then(|x| x.as_u64().or_else(|| x.as_f64().map(|n| n as u64)))
}

fn agent_listen_pids() -> Vec<u32> {
    let mut pids = Vec::new();
    #[cfg(windows)]
    {
        let mut netstat = Command::new("netstat");
        netstat.args(["-ano", "-p", "tcp"]);
        fs_viz::shell::windows_no_window(&mut netstat);
        let Ok(out) = netstat.output() else {
            return pids;
        };
        if !out.status.success() {
            return pids;
        }
        for line in String::from_utf8_lossy(&out.stdout).lines() {
            let lower = line.to_ascii_lowercase();
            if !(lower.contains("listening")) {
                continue;
            }
            let local_6007 = line.split_whitespace().any(|col| {
                let c = col.trim();
                c.ends_with(":6007")
            });
            if !local_6007 {
                continue;
            }
            if let Some(pid_s) = line.split_whitespace().last() {
                if let Ok(pid) = pid_s.parse::<u32>() {
                    if pid != 0 && !pids.contains(&pid) {
                        pids.push(pid);
                    }
                }
            }
        }
    }
    #[cfg(not(windows))]
    {
        if let Ok(out) = Command::new("lsof").args(["-ti", "tcp:6007"]).output() {
            if out.status.success() {
                for line in String::from_utf8_lossy(&out.stdout).lines() {
                    if let Ok(pid) = line.trim().parse::<u32>() {
                        if pid != 0 && !pids.contains(&pid) {
                            pids.push(pid);
                        }
                    }
                }
            }
        }
    }
    pids
}

fn stop_sidecar() {
    if let Ok(mut slot) = SIDECAR.lock() {
        if let Some(mut child) = slot.take() {
            let pid = child.id();
            host::kill_pid(pid);
            let _ = child.wait();
        }
    }
    for pid in agent_listen_pids() {
        host::kill_pid(pid);
    }
    for _ in 0..20 {
        if !sidecar_listening() {
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
}

fn sidecar_child_running() -> bool {
    if let Ok(mut slot) = SIDECAR.lock() {
        if let Some(child) = slot.as_mut() {
            return matches!(child.try_wait(), Ok(None));
        }
    }
    false
}

fn sidecar_script_fresh() -> bool {
    let want = host::sidecar_script_mtime_secs();
    match sidecar_reported_mtime_secs() {
        Some(got) if want == 0 || got == want => true,
        Some(_) => false,
        None => sidecar_child_running(),
    }
}

fn spawn_sidecar() {
    if sidecar_listening() {
        if sidecar_script_fresh() {
            return;
        }
        eprintln!("fs-viz agent sidecar script changed; recycling 127.0.0.1:6007");
        stop_sidecar();
        if sidecar_listening() {
            eprintln!("fs-viz agent sidecar still listening on 127.0.0.1:6007 after recycle");
            return;
        }
    }
    if let Ok(mut slot) = SIDECAR.lock() {
        if let Some(child) = slot.as_mut() {
            if let Ok(None) = child.try_wait() {
                return;
            }
            *slot = None;
        }
    }
    let script = host::sidecar_script();
    if !script.is_file() {
        eprintln!("fs-viz agent sidecar unavailable (no agent/server.mjs)");
        return;
    }
    let node = host::find_node();
    if node == std::path::Path::new("node") && which_missing(&node) {
        eprintln!(
            "fs-viz agent sidecar not started: Node.js not found. Install Node 22+ or set STARDUST_NODE to node.exe (portable copy used: %TEMP%\\node-portable-22\\...)."
        );
        return;
    }
    let mut sidecar = Command::new(&node);
    sidecar
        .arg(&script)
        .env("PORT", "6007")
        .current_dir(script.parent().unwrap_or_else(|| Path::new(".")))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    fs_viz::shell::windows_no_window(&mut sidecar);
    match sidecar.spawn()
    {
        Ok(mut child) => {
            for _ in 0..25 {
                if sidecar_listening() {
                    eprintln!(
                        "fs-viz agent sidecar spawned on 127.0.0.1:6007 ({})",
                        node.display()
                    );
                    if let Ok(mut slot) = SIDECAR.lock() {
                        *slot = Some(child);
                    }
                    return;
                }
                match child.try_wait() {
                    Ok(Some(st)) => {
                        eprintln!("fs-viz agent sidecar exited immediately ({st}) using {}", node.display());
                        return;
                    }
                    Err(e) => {
                        eprintln!("fs-viz agent sidecar wait failed: {e}");
                        return;
                    }
                    Ok(None) => thread::sleep(Duration::from_millis(100)),
                }
            }
            eprintln!(
                "fs-viz agent sidecar started ({}) but 127.0.0.1:6007 is not listening yet",
                node.display()
            );
            if let Ok(mut slot) = SIDECAR.lock() {
                *slot = Some(child);
            }
        }
        Err(e) => eprintln!("fs-viz agent sidecar not started ({}): {e}", node.display()),
    }
}

fn which_missing(node: &Path) -> bool {
    !node.is_file()
}

fn proxy_agent(stream: &mut TcpStream, req: &HttpReq, route_and_query: &str) {
    let addr: std::net::SocketAddr = match AGENT.parse() {
        Ok(a) => a,
        Err(_) => {
            reply_json(stream, 503, &sidecar_unavailable());
            return;
        }
    };
    let mut up = match TcpStream::connect_timeout(&addr, Duration::from_millis(800)) {
        Ok(s) => s,
        Err(_) => {
            spawn_sidecar();
            match TcpStream::connect_timeout(&addr, Duration::from_millis(800)) {
                Ok(s) => s,
                Err(_) => {
                    reply_json(stream, 503, &sidecar_unavailable());
                    return;
                }
            }
        }
    };
    let sse = route_and_query.contains("/stream");
    if sse {
        let _ = up.set_read_timeout(None);
        let _ = stream.set_read_timeout(None);
    } else {
        let _ = up.set_read_timeout(Some(Duration::from_secs(600)));
    }
    let _ = up.set_write_timeout(Some(Duration::from_secs(30)));
    let mut fwd = format!(
        "{} {} HTTP/1.1\r\nHost: 127.0.0.1:6007\r\nContent-Length: {}\r\n",
        req.method,
        route_and_query,
        req.body.len()
    );
    if sse {
        fwd.push_str("Connection: keep-alive\r\nAccept: text/event-stream\r\n");
    } else {
        fwd.push_str("Connection: close\r\n");
    }
    if let Some(ct) = header(req, "Content-Type") {
        fwd.push_str("Content-Type: ");
        fwd.push_str(ct);
        fwd.push_str("\r\n");
    }
    if let Some(ac) = header(req, "Accept") {
        if !sse {
            fwd.push_str("Accept: ");
            fwd.push_str(ac);
            fwd.push_str("\r\n");
        }
    }
    fwd.push_str("\r\n");
    if up.write_all(fwd.as_bytes()).is_err() || up.write_all(&req.body).is_err() {
        reply_json(stream, 503, &sidecar_unavailable());
        return;
    }
    let _ = up.flush();
    let mut buf = [0u8; 8192];
    let mut any = false;
    loop {
        match up.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                any = true;
                if stream.write_all(&buf[..n]).is_err() {
                    break;
                }
                let _ = stream.flush();
            }
            Err(_) => break,
        }
    }
    if !any {
        reply_json(stream, 503, &sidecar_unavailable());
    }
}

fn handle(mut stream: TcpStream) {
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
    let q = host::parse_query(&query);
    if route.starts_with("/api/cns") {
        let _ = stream.set_read_timeout(Some(Duration::from_secs(180)));
        let _ = stream.set_write_timeout(Some(Duration::from_secs(180)));
    }

    if route == "/api/agent/health" && req.method == "GET" {
        agent_health(&mut stream);
        return;
    }
    if route.starts_with("/api/agent/") || route == "/api/agent" {
        let full = agent_upstream(&route, &query);
        proxy_agent(&mut stream, &req, &full);
        return;
    }

    match (req.method.as_str(), route.as_str()) {
        ("GET", "/") => {
            serve_file(&mut stream, &host::web_dir().join("index.html"));
        }
        ("GET", "/api/meta") => reply_json(&mut stream, 200, &host::meta()),
        ("GET", "/api/children") => {
            let path = q.get("path").map(String::as_str).unwrap_or("");
            reply_api(&mut stream, host::children(path));
        }
        ("GET", "/api/file") => {
            let path = q.get("path").map(String::as_str).unwrap_or("");
            reply_api(&mut stream, host::read_file(path));
        }
        ("PUT", "/api/file") => match body_json(&req) {
            Ok(v) => match json_str(&v, "path") {
                Ok(path) => {
                    let content = v.get("content").and_then(|x| x.as_str()).unwrap_or("");
                    reply_api(&mut stream, host::write_file(path, content));
                }
                Err(e) => reply_json(&mut stream, e.status, &e.body),
            },
            Err(e) => reply_json(&mut stream, e.status, &e.body),
        },
        ("GET", "/api/search") => {
            let root = q.get("root").map(String::as_str).unwrap_or("");
            let qs = q.get("q").map(String::as_str).unwrap_or("");
            let streaming = q
                .get("stream")
                .map(|s| s == "1" || s.eq_ignore_ascii_case("true"))
                .unwrap_or(false);
            if streaming {
                match host::search_prepare(root, qs) {
                    Err(e) => reply_api(&mut stream, Err(e)),
                    Ok(()) => {
                        let _ = stream.set_write_timeout(Some(Duration::from_secs(180)));
                        let headers = "HTTP/1.1 200 OK\r\nContent-Type: application/x-ndjson; charset=utf-8\r\nCache-Control: no-cache\r\nConnection: keep-alive\r\n\r\n";
                        if stream.write_all(headers.as_bytes()).is_ok() {
                            let _ = stream.flush();
                            let _ = host::search_ndjson(root, qs, &mut stream);
                        }
                    }
                }
            } else {
                reply_api(&mut stream, host::search(root, qs));
            }
        }
        ("POST", "/api/open-external") => match body_json(&req) {
            Ok(v) => match json_str(&v, "path") {
                Ok(path) => reply_api(&mut stream, host::open_external(path)),
                Err(e) => reply_json(&mut stream, e.status, &e.body),
            },
            Err(e) => reply_json(&mut stream, e.status, &e.body),
        },
        ("GET", "/api/stat") => {
            let path = q.get("path").map(String::as_str).unwrap_or("");
            reply_api(&mut stream, host::stat(path));
        }
        ("GET", "/api/exists") => {
            let path = q.get("path").map(String::as_str).unwrap_or("");
            reply_api(&mut stream, host::exists(path));
        }
        ("GET", "/api/layout") => reply_json(&mut stream, 200, &host::get_layout()),
        ("PUT", "/api/layout") => match body_json(&req) {
            Ok(v) => reply_api(&mut stream, host::put_layout(&v)),
            Err(e) => reply_json(&mut stream, e.status, &e.body),
        },
        ("GET", "/api/agent-history") => reply_json(&mut stream, 200, &host::get_agent_history()),
        ("PUT", "/api/agent-history") => match body_json(&req) {
            Ok(v) => reply_api(&mut stream, host::put_agent_history(&v)),
            Err(e) => reply_json(&mut stream, e.status, &e.body),
        },
        ("POST", "/api/ui-pulse") => {
            fs_viz::shell::touch_ui_pulse();
            reply_json(&mut stream, 200, &json!({ "ok": true }));
        }
        ("POST", "/api/ime") => match body_json(&req) {
            Ok(v) => match host::parse_ime_composing(&v) {
                Ok(composing) => {
                    fs_viz::shell::set_ime_composing(composing);
                    reply_json(&mut stream, 200, &json!({ "ok": true }));
                }
                Err(e) => reply_json(&mut stream, e.status, &e.body),
            },
            Err(e) => reply_json(&mut stream, e.status, &e.body),
        },
        ("GET", "/api/healthz") => {
            reply_json(&mut stream, 200, &json!({ "ok": true }));
        }
        ("GET", "/api/ui-health") => {
            reply_json(&mut stream, 200, &serde_json::to_value(fs_viz::shell::probe()).unwrap_or(json!({})));
        }
        ("GET", "/api/drives") => {
            reply_json(&mut stream, 200, &json!({ "drives": fs_viz::shell::list_drives() }));
        }
        ("POST", "/api/open-explorer") => match body_json(&req) {
            Ok(v) => match json_str(&v, "path") {
                Ok(path) => match fs_viz::shell::open_in_explorer(std::path::Path::new(path)) {
                    Ok(()) => reply_json(&mut stream, 200, &json!({ "ok": true })),
                    Err(e) => reply_json(&mut stream, 500, &json!({ "error": e.to_string() })),
                },
                Err(e) => reply_json(&mut stream, e.status, &e.body),
            },
            Err(e) => reply_json(&mut stream, e.status, &e.body),
        },
        ("POST", "/api/ui-mode") => match body_json(&req) {
            Ok(v) => {
                let mode_s = v.get("mode").and_then(|x| x.as_str()).unwrap_or("");
                let Some(mode) = fs_viz::shell::UiMode::parse(mode_s) else {
                    reply_json(&mut stream, 400, &json!({ "error": "mode must be stardust or windows" }));
                    return;
                };
                let replace = v.get("replace").and_then(|x| x.as_bool()).unwrap_or(false);
                let hide = v.get("hideDesktopIcons").and_then(|x| x.as_bool()).unwrap_or(false);
                match fs_viz::shell::apply_mode(mode, replace && mode == fs_viz::shell::UiMode::Stardust, hide) {
                    Ok(file) => reply_json(
                        &mut stream,
                        200,
                        &json!({
                            "ok": true,
                            "mode": file.mode.as_str(),
                            "replaceArmed": file.replace_armed,
                            "hideDesktopIcons": file.hide_desktop_icons,
                        }),
                    ),
                    Err(e) => {
                        if replace && mode == fs_viz::shell::UiMode::Stardust {
                            match fs_viz::shell::apply_mode(mode, false, false) {
                                Ok(file) => reply_json(
                                    &mut stream,
                                    200,
                                    &json!({
                                        "ok": true,
                                        "mode": file.mode.as_str(),
                                        "replaceArmed": false,
                                        "hideDesktopIcons": false,
                                        "warning": e,
                                    }),
                                ),
                                Err(e2) => reply_json(&mut stream, 500, &json!({ "error": e2 })),
                            }
                        } else {
                            reply_json(&mut stream, 500, &json!({ "error": e }));
                        }
                    }
                }
            }
            Err(e) => reply_json(&mut stream, e.status, &e.body),
        },
        ("GET", "/api/watch") => {
            let _ = stream.set_read_timeout(None);
            let path = q.get("path").map(String::as_str).unwrap_or("");
            handle_watch(&mut stream, path);
        }
        ("POST", "/api/mkdir") => match body_json(&req) {
            Ok(v) => match json_str(&v, "path") {
                Ok(path) => reply_api(&mut stream, host::mkdir(path)),
                Err(e) => reply_json(&mut stream, e.status, &e.body),
            },
            Err(e) => reply_json(&mut stream, e.status, &e.body),
        },
        ("POST", "/api/write-new") => match body_json(&req) {
            Ok(v) => match json_str(&v, "path") {
                Ok(path) => {
                    let content = v.get("content").and_then(|x| x.as_str()).unwrap_or("");
                    reply_api(&mut stream, host::write_new(path, content));
                }
                Err(e) => reply_json(&mut stream, e.status, &e.body),
            },
            Err(e) => reply_json(&mut stream, e.status, &e.body),
        },
        ("POST", "/api/rename") => match body_json(&req) {
            Ok(v) => match (json_str(&v, "from"), json_str(&v, "to")) {
                (Ok(from), Ok(to)) => reply_api(&mut stream, host::rename(from, to)),
                (Err(e), _) | (Ok(_), Err(e)) => reply_json(&mut stream, e.status, &e.body),
            },
            Err(e) => reply_json(&mut stream, e.status, &e.body),
        },
        ("POST", "/api/delete") => match body_json(&req) {
            Ok(v) => match json_str(&v, "path") {
                Ok(path) => reply_api(&mut stream, host::delete(path)),
                Err(e) => reply_json(&mut stream, e.status, &e.body),
            },
            Err(e) => reply_json(&mut stream, e.status, &e.body),
        },
        ("POST", "/api/copy") => match body_json(&req) {
            Ok(v) => match (json_str(&v, "from"), json_str(&v, "to")) {
                (Ok(from), Ok(to)) => reply_api(&mut stream, host::copy(from, to)),
                (Err(e), _) | (Ok(_), Err(e)) => reply_json(&mut stream, e.status, &e.body),
            },
            Err(e) => reply_json(&mut stream, e.status, &e.body),
        },
        ("POST", "/api/duplicate") => match body_json(&req) {
            Ok(v) => reply_api(&mut stream, host::duplicate(&v)),
            Err(e) => reply_json(&mut stream, e.status, &e.body),
        },
        ("POST", "/api/move") => match body_json(&req) {
            Ok(v) => match (json_str(&v, "from"), json_str(&v, "to")) {
                (Ok(from), Ok(to)) => reply_api(&mut stream, host::move_path(from, to)),
                (Err(e), _) | (Ok(_), Err(e)) => reply_json(&mut stream, e.status, &e.body),
            },
            Err(e) => reply_json(&mut stream, e.status, &e.body),
        },
        ("POST", "/api/agent-out/write") => match body_json(&req) {
            Ok(v) => reply_api(&mut stream, host::agent_out_write(&v)),
            Err(e) => reply_json(&mut stream, e.status, &e.body),
        },
        ("POST", "/api/agent-out/sweep") => reply_api(&mut stream, host::sweep_agent_caps()),
        ("GET", "/api/inspect") => {
            let path = q.get("path").map(String::as_str).unwrap_or("");
            reply_api(&mut stream, host::inspect(path));
        }
        ("GET", "/api/cns/status") => reply_api(&mut stream, cns::status()),
        ("GET", "/api/cns/probe") => {
            let path = q.get("path").map(String::as_str).unwrap_or("");
            reply_api(&mut stream, cns::probe(path));
        }
        ("POST", "/api/cns/open") => match body_json(&req) {
            Ok(v) => match json_str(&v, "path") {
                Ok(path) => reply_api(&mut stream, cns::open(path)),
                Err(e) => reply_json(&mut stream, e.status, &e.body),
            },
            Err(e) => reply_json(&mut stream, e.status, &e.body),
        },
        ("GET", "/api/cns/stars") => {
            let session = q
                .get("session")
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0);
            let xmin = q.get("xmin").and_then(|s| s.parse().ok()).unwrap_or(-4000.0);
            let xmax = q.get("xmax").and_then(|s| s.parse().ok()).unwrap_or(4000.0);
            let ymin = q.get("ymin").and_then(|s| s.parse().ok()).unwrap_or(-4000.0);
            let ymax = q.get("ymax").and_then(|s| s.parse().ok()).unwrap_or(4000.0);
            let scale = q.get("z").and_then(|s| s.parse().ok()).unwrap_or(1.0);
            let focus = q.get("focus").map(String::as_str);
            let jump = q.get("jump").map(String::as_str);
            reply_api(
                &mut stream,
                cns::stars(session, xmin, xmax, ymin, ymax, scale, focus, jump),
            );
        }
        ("GET", "/api/cns/search") => {
            let session = q
                .get("session")
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0);
            let qs = q.get("q").map(String::as_str).unwrap_or("");
            reply_api(&mut stream, cns::search(session, qs));
        }
        ("GET", "/api/cns/query") => {
            let session = q
                .get("session")
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0);
            let ch = q.get("char").map(String::as_str).unwrap_or("");
            reply_api(&mut stream, cns::query(session, ch));
        }
        ("POST", "/api/cargo-test") => match body_json(&req) {
            Ok(v) => match json_str(&v, "cwd") {
                Ok(cwd) => reply_api(&mut stream, host::cargo_test(cwd)),
                Err(e) => reply_json(&mut stream, e.status, &e.body),
            },
            Err(e) => reply_json(&mut stream, e.status, &e.body),
        },
        ("POST", "/api/rebuild-ui") => {
            let parsed = if req.body.is_empty() {
                Ok(json!({}))
            } else {
                body_json(&req)
            };
            match parsed {
                Ok(v) => match host::parse_rebuild_features(&v) {
                    Ok(features) => reply_api(&mut stream, host::rebuild_ui(&features)),
                    Err(e) => reply_json(&mut stream, e.status, &e.body),
                },
                Err(e) => reply_json(&mut stream, e.status, &e.body),
            }
        }
        ("POST", "/api/open-cursor") => reply_api(&mut stream, host::open_cursor()),
        ("POST", "/api/workspace") => match body_json(&req) {
            Ok(v) => {
                let roots = v
                    .get("roots")
                    .and_then(|x| x.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|x| x.as_str().map(|s| s.to_string()))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                match host::workspace(&roots) {
                    Ok(body) => {
                        if body.get("cwd").and_then(|x| x.as_str()).is_some() {
                            notify_sidecar_workspace(&body);
                        }
                        reply_json(&mut stream, 200, &body);
                    }
                    Err(e) => reply_json(&mut stream, e.status, &e.body),
                }
            }
            Err(e) => reply_json(&mut stream, e.status, &e.body),
        },
        ("GET", "/api/ui/inbox") => reply_json(&mut stream, 200, &host::ui_inbox_take()),
        ("POST", "/api/ui/command") => match body_json(&req) {
            Ok(v) => {
                let op = v.get("op").and_then(|x| x.as_str()).unwrap_or("");
                let args = v.get("args").cloned().unwrap_or_else(|| json!({}));
                reply_api(&mut stream, host::ui_enqueue(op, args));
            }
            Err(e) => reply_json(&mut stream, e.status, &e.body),
        },
        ("POST", "/api/ui/result") => match body_json(&req) {
            Ok(v) => reply_api(&mut stream, host::ui_post_result(&v)),
            Err(e) => reply_json(&mut stream, e.status, &e.body),
        },
        ("POST", "/api/drive") => match body_json(&req) {
            Ok(v) => {
                #[cfg(windows)]
                {
                    reply_api(&mut stream, desktop_drive::command(&v));
                }
                #[cfg(not(windows))]
                {
                    reply_json(
                        &mut stream,
                        400,
                        &json!({ "error": "desktop drive is Windows-only" }),
                    );
                }
            }
            Err(e) => reply_json(&mut stream, e.status, &e.body),
        },
        ("GET", route) if route.starts_with("/web/") => {
            let rel = route.trim_start_matches("/web/");
            serve_web(&mut stream, rel);
        }
        ("GET", route) if static_ext(route) => {
            let rel = route.trim_start_matches('/');
            serve_web(&mut stream, rel);
        }
        _ => reply_json(&mut stream, 404, &json!({ "error": "not found" })),
    }
}

fn parse_cli() -> Cli {
    let mut cli = Cli::default();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--headless" | "--serve" => cli.headless = true,
            "--desktop" => cli.desktop = true,
            "--open" => {
                if let Some(p) = args.next() {
                    cli.open = Some(p);
                }
            }
            "--replace" => cli.replace = true,
            "-h" | "--help" => {
                eprintln!(
                    "fs-viz [--headless] [--open PATH] [--desktop]\n  default: HTTP server on {}\n  --open/--desktop: Edge/WebView2 app window",
                    fs_viz::shell::BIND
                );
                std::process::exit(0);
            }
            other if !other.starts_with('-') && cli.open.is_none() => cli.open = Some(other.to_string()),
            _ => {}
        }
    }
    cli
}

#[derive(Default)]
struct Cli {
    headless: bool,
    desktop: bool,
    open: Option<String>,
    replace: bool,
}

fn spawn_server() -> bool {
    if fs_viz::shell::http_ok() {
        return true;
    }
    spawn_sidecar();
    thread::spawn(|| loop {
        fs_viz::shell::touch_heartbeat();
        thread::sleep(Duration::from_secs(1));
    });
    match TcpListener::bind(BIND) {
        Ok(listener) => {
            SERVER_OWNED.store(true, std::sync::atomic::Ordering::SeqCst);
            eprintln!(
                "fs-viz cosmos {}",
                host::path_to_string(&host::session_root())
            );
            eprintln!("fs-viz http://{BIND}");
            thread::spawn(move || {
                for stream in listener.incoming() {
                    if let Ok(stream) = stream {
                        thread::spawn(move || handle(stream));
                    }
                }
            });
            true
        }
        Err(e) => {
            if fs_viz::shell::http_ok() {
                true
            } else {
                eprintln!("fs-viz bind {BIND}: {e}");
                false
            }
        }
    }
}

fn park() {
    loop {
        thread::sleep(Duration::from_secs(60));
    }
}

fn main() {
    fs_viz::shell::enable_dpi_aware();
    let cli = parse_cli();
    if cli.headless || cli.desktop || cli.open.is_some() {
        fs_viz::shell::detach_console();
    }
    host::load_local_env();
    if cli.replace {
        match fs_viz::shell::apply_mode(fs_viz::shell::UiMode::Stardust, true, true) {
            Ok(_) => eprintln!(
                "fs-viz Explorer replacement armed (failsafe {})",
                fs_viz::shell::FAILSAFE_HOTKEY
            ),
            Err(e) => eprintln!("fs-viz replace failed: {e}"),
        }
    }
    let want_window = cli.desktop || cli.open.is_some();
    if !spawn_server() {
        if !(want_window
            && fs_viz::shell::wait_http(Duration::from_secs(
                fs_viz::shell::DESKTOP_HTTP_WAIT_SECS,
            )))
        {
            std::process::exit(1);
        }
    }
    if want_window {
        if cli.desktop {
            fs_viz::shell::ensure_login_autostart();
        }
        let _ = fs_viz::shell::wait_http(Duration::from_secs(
            fs_viz::shell::DESKTOP_HTTP_WAIT_SECS,
        ));
        let url = fs_viz::shell::native_window_url(
            cli.desktop,
            cli.open.as_deref().map(Path::new),
            fs_viz::shell::load_mode().mode,
        );
        if let Err(e) = fs_viz::shell::open_webview_url(&url) {
            eprintln!("fs-viz native window: {e} — open {url} in a browser");
        }
        if SERVER_OWNED.load(std::sync::atomic::Ordering::SeqCst) {
            park();
        } else if cli.desktop {
            let _ = fs_viz::shell::wait_http(Duration::from_secs(3));
            let _ = fs_viz::shell::wait_stardust_hwnd(Duration::from_secs(10));
        }
        return;
    }
    if !SERVER_OWNED.load(std::sync::atomic::Ordering::SeqCst) {
        eprintln!("fs-viz already running at http://{BIND}");
        return;
    }
    park();
}

static SERVER_OWNED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

