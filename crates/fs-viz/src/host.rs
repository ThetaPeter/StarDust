//! Host filesystem helpers for the fs-viz HTTP API. Not the synthetic StarDust tree.

use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde_json::{json, Value};
use stardust::{Class, Field, Import, Ir, Param, Ret, Type};
use notify::{Config, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

const FILE_CAP: u64 = 2 * 1024 * 1024;
const TEXT_PROBE: usize = 8 * 1024;
const SEARCH_CAP: usize = 200;
const SEARCH_VISIT_CAP: usize = 100_000;
const SEARCH_EXTRA_VISIT_CAP: usize = 12_000;
const EXISTS_CHILD_CAP: usize = 40;
const CARGO_TIMEOUT: Duration = Duration::from_secs(600);

static WS_SEQ: AtomicU64 = AtomicU64::new(1);
static WORKSPACE: Mutex<Option<(PathBuf, Vec<String>, Vec<PathBuf>)>> = Mutex::new(None);
static UI_SEQ: AtomicU64 = AtomicU64::new(1);
const UI_CMD_TIMEOUT: Duration = Duration::from_secs(8);
const UI_INBOX_CAP: usize = 8;

struct UiCommand {
    id: String,
    op: String,
    args: Value,
}

struct UiInbox {
    pending: Vec<UiCommand>,
    waiters: HashMap<String, mpsc::Sender<Value>>,
}

static UI_INBOX: Mutex<Option<UiInbox>> = Mutex::new(None);

const BINARY_EXT: &[&str] = &[
    "exe", "dll", "png", "jpg", "jpeg", "gif", "webp", "pdf", "zip", "7z", "obj", "lib", "pdb",
    "wasm", "so", "dylib", "o", "a", "bin",
];

#[derive(Debug)]
pub struct ApiError {
    pub status: u16,
    pub body: Value,
}

pub type ApiResult = Result<Value, ApiError>;

impl ApiError {
    pub fn bad(msg: impl Into<String>) -> Self {
        Self {
            status: 400,
            body: json!({ "error": msg.into() }),
        }
    }

    pub fn not_found() -> Self {
        Self {
            status: 404,
            body: json!({ "error": "not found" }),
        }
    }

    pub fn conflict(msg: impl Into<String>) -> Self {
        Self {
            status: 409,
            body: json!({ "error": msg.into() }),
        }
    }

    pub fn internal(msg: impl Into<String>) -> Self {
        Self {
            status: 500,
            body: json!({ "error": msg.into() }),
        }
    }
}

fn io_err(e: io::Error) -> ApiError {
    if e.kind() == io::ErrorKind::NotFound {
        ApiError::not_found()
    } else {
        ApiError::internal(e.to_string())
    }
}

pub fn path_to_string(path: &Path) -> String {
    let s = path.to_string_lossy().into_owned();
    if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else if let Some(rest) = s.strip_prefix(r"\\?\") {
        rest.to_string()
    } else {
        s
    }
}

pub fn parse_abs(s: &str) -> Result<PathBuf, ApiError> {
    if s.is_empty() {
        return Err(ApiError::bad("missing path"));
    }
    let p = PathBuf::from(s);
    if !p.is_absolute() {
        return Err(ApiError::bad("path must be absolute"));
    }
    Ok(p)
}

pub fn canonicalize_if_exists(p: PathBuf) -> PathBuf {
    if p.exists() {
        p.canonicalize().unwrap_or(p)
    } else {
        p
    }
}

pub fn resolve_abs(s: &str) -> Result<PathBuf, ApiError> {
    Ok(canonicalize_if_exists(parse_abs(s)?))
}

fn require_exists(p: &Path) -> Result<(), ApiError> {
    if p.exists() {
        Ok(())
    } else {
        Err(ApiError::not_found())
    }
}

pub fn default_root() -> PathBuf {
    let p = fs_viz::World::demo_root();
    p.canonicalize().unwrap_or(p)
}

pub fn session_root() -> PathBuf {
    fs_viz::shell::session_root(&default_root())
}

pub fn meta() -> Value {
    let root = default_root();
    let extra = fs_viz::shell::meta_json(&path_to_string(&root));
    let mut v = json!({
        "defaultRoot": path_to_string(&root),
        "sessionRoot": extra.get("sessionRoot").cloned().unwrap_or(json!(path_to_string(&session_root()))),
        "layoutPath": path_to_string(&layout_file()),
        "historyPath": path_to_string(&history_file()),
        "hasCursorKey": has_cursor_key(),
    });
    if let Some(obj) = v.as_object_mut() {
        if let Some(map) = extra.as_object() {
            for (k, val) in map {
                obj.insert(k.clone(), val.clone());
            }
        }
    }
    v
}

pub fn layout_file() -> PathBuf {
    #[cfg(windows)]
    {
        if let Some(appdata) = std::env::var_os("APPDATA") {
            return PathBuf::from(appdata).join("StarDust").join("ui-layout.json");
        }
    }
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
        return PathBuf::from(xdg).join("stardust").join("ui-layout.json");
    }
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"));
    if let Some(home) = home {
        return PathBuf::from(home)
            .join(".config")
            .join("stardust")
            .join("ui-layout.json");
    }
    PathBuf::from("stardust-ui-layout.json")
}

pub fn history_file() -> PathBuf {
    layout_file().with_file_name("agent-history.json")
}

pub fn load_local_env() {
    let mut files = vec![PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".env")];
    files.push(default_root().join(".env"));
    for f in files {
        let Ok(text) = fs::read_to_string(&f) else {
            continue;
        };
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            let key = k.trim();
            if key.is_empty() || std::env::var_os(key).is_some() {
                continue;
            }
            let val = v.trim().trim_matches(|c| c == '"' || c == '\'');
            std::env::set_var(key, val);
        }
    }
}

pub fn has_cursor_key() -> bool {
    std::env::var("CURSOR_API_KEY")
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false)
}

fn unix_secs(t: io::Result<SystemTime>) -> u64 {
    t.ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn is_binary_ext(path: &Path) -> bool {
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    BINARY_EXT.contains(&ext.as_str())
}

pub fn is_text_file(path: &Path) -> bool {
    if path.is_dir() {
        return false;
    }
    if is_binary_ext(path) {
        return false;
    }
    let mut f = match File::open(path) {
        Ok(f) => f,
        Err(_) => return false,
    };
    let mut buf = [0u8; TEXT_PROBE];
    let n = match f.read(&mut buf) {
        Ok(n) => n,
        Err(_) => return false,
    };
    let slice = &buf[..n];
    if slice.contains(&0) {
        return false;
    }
    std::str::from_utf8(slice).is_ok()
}

fn entry_kind(path: &Path) -> &'static str {
    match fs::symlink_metadata(path) {
        Ok(m) if m.file_type().is_dir() => "dir",
        Ok(_) => {
            if path.is_dir() {
                "dir"
            } else {
                "file"
            }
        }
        Err(_) => {
            if path.is_dir() {
                "dir"
            } else {
                "file"
            }
        }
    }
}

#[derive(Serialize)]
struct Entry {
    name: String,
    kind: String,
    size: u64,
    mtime: u64,
    #[serde(rename = "isText")]
    is_text: bool,
    /// Absolute path for overlay entries (Public Desktop). Omitted for the listed folder's own children.
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<String>,
}

fn same_dir(a: &Path, b: &Path) -> bool {
    let a = path_to_string(&canonicalize_if_exists(a.to_path_buf()));
    let b = path_to_string(&canonicalize_if_exists(b.to_path_buf()));
    a.replace('\\', "/").eq_ignore_ascii_case(&b.replace('\\', "/"))
}

fn entry_name_key(name: &str) -> String {
    #[cfg(windows)]
    {
        name.to_lowercase()
    }
    #[cfg(not(windows))]
    {
        name.to_string()
    }
}

fn is_user_desktop(path: &Path) -> bool {
    same_dir(path, &fs_viz::shell::desktop_dir())
}

fn dir_entry_from(ent: fs::DirEntry, include_abs_path: bool) -> Option<Entry> {
    let name = ent.file_name().to_string_lossy().into_owned();
    let child = ent.path();
    let meta = fs::symlink_metadata(&child).ok()?;
    let kind = entry_kind(&child);
    let size = meta.len();
    let mtime = unix_secs(meta.modified());
    let is_text = kind == "file" && is_text_file(&child);
    Some(Entry {
        name,
        kind: kind.into(),
        size,
        mtime,
        is_text,
        path: if include_abs_path {
            Some(path_to_string(&child))
        } else {
            None
        },
    })
}

fn collect_dir_entries(dir: &Path, include_abs_path: bool) -> Result<Vec<Entry>, ApiError> {
    let rd = fs::read_dir(dir).map_err(io_err)?;
    let mut entries = Vec::new();
    for ent in rd {
        let Ok(ent) = ent else { continue };
        if let Some(e) = dir_entry_from(ent, include_abs_path) {
            entries.push(e);
        }
    }
    Ok(entries)
}

fn overlay_named_entries(entries: &mut Vec<Entry>, extra_dir: &Path) {
    if !extra_dir.is_dir() {
        return;
    }
    let mut seen: HashSet<String> = entries.iter().map(|e| entry_name_key(&e.name)).collect();
    let extra = match collect_dir_entries(extra_dir, true) {
        Ok(v) => v,
        Err(_) => return,
    };
    for e in extra {
        if !seen.insert(entry_name_key(&e.name)) {
            continue;
        }
        entries.push(e);
    }
}

fn sort_entries(entries: &mut [Entry]) {
    entries.sort_by(|a, b| match (a.kind.as_str() == "dir", b.kind.as_str() == "dir") {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.name.cmp(&b.name),
    });
}

/// Explorer-like: user Desktop known folder plus `%PUBLIC%\Desktop`. User names win.
fn merge_public_desktop(listed: &Path, entries: &mut Vec<Entry>) {
    if !is_user_desktop(listed) {
        return;
    }
    let Some(public) = fs_viz::shell::public_desktop_dir() else {
        return;
    };
    if same_dir(listed, &public) {
        return;
    }
    overlay_named_entries(entries, &public);
}

pub fn children(path: &str) -> ApiResult {
    let path = resolve_abs(path)?;
    require_exists(&path)?;
    if !path.is_dir() {
        return Err(ApiError::bad("not a directory"));
    }
    let mut entries = collect_dir_entries(&path, false)?;
    merge_public_desktop(&path, &mut entries);
    sort_entries(&mut entries);
    Ok(json!({
        "path": path_to_string(&path),
        "entries": entries,
    }))
}

pub fn read_file(path: &str) -> ApiResult {
    let path = resolve_abs(path)?;
    require_exists(&path)?;
    if path.is_dir() {
        return Err(ApiError::bad("not a file"));
    }
    let meta = fs::metadata(&path).map_err(io_err)?;
    if meta.len() > FILE_CAP {
        return Err(ApiError {
            status: 413,
            body: json!({ "error": "file too large" }),
        });
    }
    if !is_text_file(&path) {
        return Err(ApiError {
            status: 415,
            body: json!({ "error": "binary file", "binary": true }),
        });
    }
    let bytes = fs::read(&path).map_err(io_err)?;
    match String::from_utf8(bytes) {
        Ok(content) => Ok(json!({
            "path": path_to_string(&path),
            "content": content,
            "binary": false,
        })),
        Err(_) => Err(ApiError {
            status: 415,
            body: json!({ "error": "binary file", "binary": true }),
        }),
    }
}

pub fn write_file(path: &str, content: &str) -> ApiResult {
    let path = resolve_abs(path)?;
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            fs::create_dir_all(parent).map_err(io_err)?;
        }
    }
    fs::write(&path, content.as_bytes()).map_err(io_err)?;
    Ok(json!({ "ok": true }))
}

#[derive(Serialize)]
struct Hit {
    path: String,
    name: String,
    parent: String,
    kind: String,
}

fn search_skip_descend(name: &str) -> bool {
    name.eq_ignore_ascii_case("target")
        || name.eq_ignore_ascii_case("node_modules")
        || name.eq_ignore_ascii_case(".git")
        || name.eq_ignore_ascii_case("dist")
        || name.eq_ignore_ascii_case("__pycache__")
        || name.eq_ignore_ascii_case("AppData")
        || name.eq_ignore_ascii_case("Application Data")
}

fn strip_query_seps(q: &str) -> &str {
    let t = q.trim();
    if t.len() == 3 {
        let b = t.as_bytes();
        if b[1] == b':' && (b[2] == b'\\' || b[2] == b'/') {
            return t;
        }
    }
    t.trim_end_matches(['/', '\\'])
}

fn path_jump_candidate(root: &Path, q: &str) -> Option<PathBuf> {
    let q = strip_query_seps(q);
    if q.is_empty() || q == "." || q == ".." {
        return None;
    }
    let cand = if Path::new(q).is_absolute() {
        PathBuf::from(q)
    } else {
        root.join(q)
    };
    if cand.exists() {
        Some(cand)
    } else {
        None
    }
}

fn hit_from_path(path: &Path) -> Hit {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path_to_string(path));
    let parent = path.parent().map(path_to_string).unwrap_or_default();
    let kind = if path.is_dir() { "dir" } else { "file" };
    Hit {
        path: path_to_string(path),
        name,
        parent,
        kind: kind.into(),
    }
}

fn path_lc_key(path: &Path) -> String {
    path_to_string(path).replace('\\', "/").to_lowercase()
}

fn extra_search_roots() -> Vec<PathBuf> {
    let mut out = Vec::new();
    #[cfg(windows)]
    {
        if let Ok(home) = std::env::var("USERPROFILE") {
            let home = PathBuf::from(home);
            out.push(home.join("Desktop"));
            out.push(home.join("OneDrive").join("Desktop"));
        }
        if let Ok(public) = std::env::var("PUBLIC") {
            out.push(PathBuf::from(public).join("Desktop"));
        }
    }
    #[cfg(not(windows))]
    {
        if let Ok(home) = std::env::var("HOME") {
            out.push(PathBuf::from(home).join("Desktop"));
        }
    }
    out
}

fn search_score(h: &Hit, q_lc: &str, root: &Path, jump: Option<&Path>) -> i32 {
    let name = h.name.to_lowercase();
    let path = h.path.replace('\\', "/").to_lowercase();
    let root_s = path_to_string(root).replace('\\', "/").to_lowercase();
    let mut s = 0i32;
    if let Some(jp) = jump {
        if path.trim_end_matches('/') == path_lc_key(jp).trim_end_matches('/') {
            s += 100_000;
        }
    }
    if path.trim_end_matches('/') == root_s.trim_end_matches('/') {
        s += 50_000;
    }
    if name == q_lc {
        s += 8_000;
    } else if name.starts_with(q_lc) {
        s += 4_000;
    } else if name.contains(q_lc) {
        s += 1_000;
    }
    if h.kind == "dir" {
        s += 500;
    }
    let depth = path.chars().filter(|&c| c == '/' || c == '\\').count() as i32;
    s -= depth * 20;
    s
}

fn emit_hit(hits: &mut Vec<Hit>, h: Hit, emit: &mut dyn FnMut(&Hit)) {
    emit(&h);
    hits.push(h);
}

pub fn search_prepare(root: &str, q: &str) -> Result<(), ApiError> {
    if q.is_empty() {
        return Ok(());
    }
    let root = resolve_abs(root)?;
    reject_forbidden_move(&root)?;
    require_exists(&root)?;
    if !root.is_dir() {
        return Err(ApiError::bad("root must be a directory"));
    }
    Ok(())
}

fn search_hits(root: &str, q: &str, emit: &mut dyn FnMut(&Hit)) -> Result<Vec<Hit>, ApiError> {
    if q.is_empty() {
        return Ok(Vec::new());
    }
    let root = resolve_abs(root)?;
    reject_forbidden_move(&root)?;
    require_exists(&root)?;
    if !root.is_dir() {
        return Err(ApiError::bad("root must be a directory"));
    }
    let q_lc = q.to_lowercase();
    let mut hits = Vec::new();
    let jump = path_jump_candidate(&root, q);
    if let Some(ref p) = jump {
        if !forbidden_path(p) {
            emit_hit(&mut hits, hit_from_path(p), emit);
        }
    }
    let mut visited = HashSet::new();
    let mut visits = 0usize;
    let root_name = root
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    if !root_name.is_empty() && root_name.to_lowercase().contains(&q_lc) {
        let parent = root
            .parent()
            .map(path_to_string)
            .unwrap_or_default();
        emit_hit(
            &mut hits,
            Hit {
                path: path_to_string(&root),
                name: root_name,
                parent,
                kind: "dir".into(),
            },
            emit,
        );
    }
    search_walk(
        &root,
        &q_lc,
        &mut hits,
        &mut visited,
        &mut visits,
        SEARCH_VISIT_CAP,
        SEARCH_CAP,
        emit,
    );
    let root_key = root.canonicalize().unwrap_or_else(|_| root.clone());
    for extra in extra_search_roots() {
        if !extra.is_dir() {
            continue;
        }
        let extra_key = extra.canonicalize().unwrap_or_else(|_| extra.clone());
        if extra_key == root_key {
            continue;
        }
        let extra_name = extra
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if !extra_name.is_empty() && extra_name.to_lowercase().contains(&q_lc) {
            let parent = extra.parent().map(path_to_string).unwrap_or_default();
            emit_hit(
                &mut hits,
                Hit {
                    path: path_to_string(&extra),
                    name: extra_name,
                    parent,
                    kind: "dir".into(),
                },
                emit,
            );
        }
        let mut extra_visits = 0usize;
        search_walk(
            &extra,
            &q_lc,
            &mut hits,
            &mut visited,
            &mut extra_visits,
            SEARCH_EXTRA_VISIT_CAP,
            SEARCH_CAP + 80,
            emit,
        );
    }
    {
        let mut seen = HashSet::new();
        hits.retain(|h| seen.insert(h.path.replace('\\', "/").to_lowercase()));
    }
    hits.sort_by(|a, b| {
        search_score(b, &q_lc, &root, jump.as_deref())
            .cmp(&search_score(a, &q_lc, &root, jump.as_deref()))
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    if hits.len() > SEARCH_CAP {
        hits.truncate(SEARCH_CAP);
    }
    Ok(hits)
}

pub fn search(root: &str, q: &str) -> ApiResult {
    let hits = search_hits(root, q, &mut |_| {})?;
    Ok(json!({ "hits": hits }))
}

pub fn search_ndjson(root: &str, q: &str, w: &mut dyn Write) -> ApiResult {
    let hits = search_hits(root, q, &mut |h| {
        let _ = writeln!(w, "{}", json!({ "hit": h }));
        let _ = w.flush();
    })?;
    let _ = writeln!(w, "{}", json!({ "done": true, "hits": hits }));
    let _ = w.flush();
    Ok(json!({ "ok": true }))
}

fn search_walk(
    dir: &Path,
    q_lc: &str,
    hits: &mut Vec<Hit>,
    visited: &mut HashSet<PathBuf>,
    visits: &mut usize,
    visit_cap: usize,
    hit_cap: usize,
    emit: &mut dyn FnMut(&Hit),
) {
    if hits.len() >= hit_cap || *visits >= visit_cap {
        return;
    }
    let key = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
    if !visited.insert(key) {
        return;
    }
    let rd = match fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(_) => return,
    };
    let mut ents: Vec<_> = rd.filter_map(|e| e.ok()).collect();
    ents.sort_by(|a, b| a.file_name().cmp(&b.file_name()));
    let mut recurse_match: Vec<PathBuf> = Vec::new();
    let mut recurse_other: Vec<PathBuf> = Vec::new();
    for ent in ents {
        if hits.len() >= hit_cap || *visits >= visit_cap {
            return;
        }
        *visits += 1;
        let name = ent.file_name().to_string_lossy().into_owned();
        let child = ent.path();
        let kind = entry_kind(&child);
        let name_lc = name.to_lowercase();
        if name_lc.contains(q_lc) {
            emit_hit(
                &mut *hits,
                Hit {
                    path: path_to_string(&child),
                    name: name.clone(),
                    parent: path_to_string(dir),
                    kind: kind.into(),
                },
                emit,
            );
        }
        if kind == "dir" && !search_skip_descend(&name) {
            if name_lc.contains(q_lc) {
                recurse_match.push(child);
            } else {
                recurse_other.push(child);
            }
        }
    }
    for child in recurse_match.into_iter().chain(recurse_other) {
        if hits.len() >= hit_cap || *visits >= visit_cap {
            return;
        }
        search_walk(
            child.as_path(),
            q_lc,
            hits,
            visited,
            visits,
            visit_cap,
            hit_cap,
            emit,
        );
    }
}

pub fn open_external(path: &str) -> ApiResult {
    let path = resolve_abs(path)?;
    reject_forbidden_move(&path)?;
    require_exists(&path)?;
    if path.is_dir() {
        return Err(ApiError::bad("path is a directory; use jump"));
    }
    let path_s = path_to_string(&path);
    #[cfg(windows)]
    {
        let mut cmd = Command::new("cmd");
        cmd.args(["/C", "start", "", &path_s]);
        fs_viz::shell::windows_no_window(&mut cmd);
        let child = cmd
            .spawn()
            .map_err(|e| ApiError::internal(e.to_string()))?;
        drop(child);
    }
    #[cfg(not(windows))]
    {
        Command::new("xdg-open")
            .arg(&path_s)
            .spawn()
            .map_err(|e| ApiError::internal(e.to_string()))?;
    }
    Ok(json!({ "ok": true }))
}

pub fn stat(path: &str) -> ApiResult {
    let path = resolve_abs(path)?;
    require_exists(&path)?;
    let meta = fs::metadata(&path).map_err(io_err)?;
    let kind = if path.is_dir() { "dir" } else { "file" };
    Ok(json!({
        "path": path_to_string(&path),
        "kind": kind,
        "size": meta.len(),
        "mtime": unix_secs(meta.modified()),
        "created": unix_secs(meta.created()),
        "isText": kind == "file" && is_text_file(&path),
    }))
}

fn child_names(dir: &Path, cap: usize) -> Vec<String> {
    let mut names: Vec<String> = match fs::read_dir(dir) {
        Ok(rd) => rd
            .filter_map(|e| e.ok())
            .filter_map(|e| e.file_name().into_string().ok())
            .collect(),
        Err(_) => return Vec::new(),
    };
    names.sort_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase()));
    if names.len() > cap {
        names.truncate(cap);
    }
    names
}

/// Existence without navigating the cosmos. Missing → `{ exists: false }` (not 404).
pub fn exists(path: &str) -> ApiResult {
    let path = resolve_abs(path)?;
    reject_forbidden_move(&path)?;
    if !path.exists() {
        return Ok(json!({ "exists": false }));
    }
    let kind = if path.is_dir() { "dir" } else { "file" };
    if kind == "dir" {
        Ok(json!({
            "exists": true,
            "kind": "dir",
            "children": child_names(&path, EXISTS_CHILD_CAP),
        }))
    } else {
        Ok(json!({ "exists": true, "kind": "file" }))
    }
}

pub fn get_layout() -> Value {
    let p = layout_file();
    let Ok(text) = fs::read_to_string(&p) else {
        return json!({});
    };
    match serde_json::from_str::<Value>(&text) {
        Ok(Value::Object(map)) => Value::Object(map),
        _ => json!({}),
    }
}

pub fn put_layout(value: &Value) -> ApiResult {
    if !value.is_object() {
        return Err(ApiError::bad("layout body must be a JSON object"));
    }
    let p = layout_file();
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent).map_err(io_err)?;
    }
    let pretty = serde_json::to_string_pretty(value).map_err(|e| ApiError::bad(e.to_string()))?;
    fs::write(&p, pretty).map_err(io_err)?;
    Ok(json!({ "ok": true }))
}

/// `POST /api/ime` body: `{ "composing": true|false }`. Missing or non-bool → 400.
pub fn parse_ime_composing(v: &Value) -> Result<bool, ApiError> {
    match v.get("composing") {
        None => Err(ApiError::bad("composing required")),
        Some(x) => x
            .as_bool()
            .ok_or_else(|| ApiError::bad("composing must be a bool")),
    }
}

pub fn get_agent_history() -> Value {
    let p = history_file();
    let Ok(text) = fs::read_to_string(&p) else {
        return json!({});
    };
    match serde_json::from_str::<Value>(&text) {
        Ok(Value::Object(map)) => Value::Object(map),
        _ => json!({}),
    }
}

pub fn put_agent_history(value: &Value) -> ApiResult {
    if !value.is_object() {
        return Err(ApiError::bad("history body must be a JSON object"));
    }
    let p = history_file();
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent).map_err(io_err)?;
    }
    let pretty = serde_json::to_string_pretty(value).map_err(|e| ApiError::bad(e.to_string()))?;
    fs::write(&p, pretty).map_err(io_err)?;
    Ok(json!({ "ok": true }))
}

pub fn mkdir(path: &str) -> ApiResult {
    let path = resolve_abs(path)?;
    reject_forbidden_move(&path)?;
    if path.exists() {
        return Err(ApiError::conflict("path already exists"));
    }
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            return Err(ApiError::bad("parent directory does not exist"));
        }
    }
    fs::create_dir(&path).map_err(io_err)?;
    Ok(json!({ "ok": true }))
}

pub fn write_new(path: &str, content: &str) -> ApiResult {
    let path = resolve_abs(path)?;
    if path.exists() {
        return Err(ApiError::conflict("path already exists"));
    }
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            return Err(ApiError::bad("parent directory does not exist"));
        }
    }
    fs::write(&path, content.as_bytes()).map_err(io_err)?;
    Ok(json!({ "ok": true }))
}

pub fn rename(from: &str, to: &str) -> ApiResult {
    let from = resolve_abs(from)?;
    let to = resolve_abs(to)?;
    require_exists(&from)?;
    if to.exists() {
        return Err(ApiError::conflict("destination already exists"));
    }
    fs::rename(&from, &to).map_err(io_err)?;
    Ok(json!({ "ok": true }))
}

pub fn delete(path: &str) -> ApiResult {
    let path = resolve_abs(path)?;
    require_exists(&path)?;
    trash::delete(&path).map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(json!({ "ok": true }))
}

fn copy_recursive(from: &Path, to: &Path) -> Result<(), ApiError> {
    if from.is_dir() {
        fs::create_dir_all(to).map_err(io_err)?;
        for ent in fs::read_dir(from).map_err(io_err)? {
            let ent = ent.map_err(io_err)?;
            copy_recursive(&ent.path(), &to.join(ent.file_name()))?;
        }
        Ok(())
    } else {
        if let Some(parent) = to.parent() {
            if !parent.as_os_str().is_empty() && !parent.exists() {
                fs::create_dir_all(parent).map_err(io_err)?;
            }
        }
        fs::copy(from, to).map_err(io_err)?;
        Ok(())
    }
}

pub fn copy(from: &str, to: &str) -> ApiResult {
    let from = resolve_abs(from)?;
    let to = resolve_abs(to)?;
    require_exists(&from)?;
    if to.exists() {
        return Err(ApiError::conflict("destination already exists"));
    }
    copy_recursive(&from, &to)?;
    Ok(json!({ "ok": true }))
}

fn is_stardust_ws_path(p: &Path) -> bool {
    p.components().any(|c| {
        c.as_os_str()
            .to_string_lossy()
            .to_ascii_lowercase()
            .starts_with("stardust-ws-")
    })
}

fn is_appdata_component(p: &Path) -> bool {
    p.components().any(|c| {
        let n = c.as_os_str().to_string_lossy();
        n.eq_ignore_ascii_case("AppData") || n.eq_ignore_ascii_case("Application Data")
    })
}

fn forbidden_path(p: &Path) -> bool {
    is_stardust_ws_path(p) || is_appdata_component(p)
}

fn reject_forbidden_move(p: &Path) -> Result<(), ApiError> {
    if forbidden_path(p) {
        return Err(ApiError::bad("refuses AppData / agent temp workspace"));
    }
    Ok(())
}

fn reject_temp_workspace_dest(p: &Path) -> Result<(), ApiError> {
    if is_stardust_ws_path(p) {
        return Err(ApiError::bad(
            "destination is the agent temp workspace; use a real host folder such as Desktop",
        ));
    }
    Ok(())
}

fn overlay_to_real(p: PathBuf) -> PathBuf {
    let Ok(guard) = WORKSPACE.lock() else {
        return p;
    };
    let Some((cwd, names, reals)) = guard.as_ref() else {
        return p;
    };
    let a = path_to_string(&p).replace('\\', "/");
    let b = path_to_string(cwd).replace('\\', "/");
    let a_l = a.to_ascii_lowercase();
    let b_trim = b.trim_end_matches('/');
    let b_l = b_trim.to_ascii_lowercase();
    if a_l == b_l {
        return p;
    }
    if !a_l.starts_with(&format!("{b_l}/")) {
        return p;
    }
    let rest = a[b_trim.len()..].trim_start_matches('/');
    let segs: Vec<&str> = rest.split('/').filter(|s| !s.is_empty()).collect();
    let Some(first) = segs.first() else {
        return p;
    };
    for (name, real) in names.iter().zip(reals.iter()) {
        if name.eq_ignore_ascii_case(first) {
            let mut out = real.clone();
            for part in &segs[1..] {
                out.push(*part);
            }
            return out;
        }
    }
    p
}

fn resolve_duplicate_source(s: &str) -> Result<PathBuf, ApiError> {
    let mapped = overlay_to_real(parse_abs(s)?);
    let path = canonicalize_if_exists(mapped);
    require_exists(&path)?;
    Ok(path)
}

fn split_file_name(name: &str) -> (&str, &str) {
    if let Some(dot) = name.rfind('.') {
        if dot > 0 {
            return (&name[..dot], &name[dot..]);
        }
    }
    (name, "")
}

/// Explorer-style duplicate name: `stem - Copy.ext`, then `stem - Copy (2).ext`.
pub fn explorer_copy_candidate(original_name: &str, n: u32) -> String {
    let (stem, ext) = split_file_name(original_name);
    if n <= 1 {
        format!("{stem} - Copy{ext}")
    } else {
        format!("{stem} - Copy ({n}){ext}")
    }
}

fn unique_explorer_name(dir: &Path, original_name: &str) -> String {
    let mut n = 1u32;
    loop {
        let cand = explorer_copy_candidate(original_name, n);
        if !dir.join(&cand).exists() {
            return cand;
        }
        if n == u32::MAX || n > 10_000 {
            return cand;
        }
        n += 1;
    }
}

fn dest_file_path(from: &Path, dest_dir: &Path) -> Result<PathBuf, ApiError> {
    let name = from
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .ok_or_else(|| ApiError::bad("source has no file name"))?;
    let candidate = dest_dir.join(&name);
    let same = from.parent().map(|p| same_dir(p, dest_dir)).unwrap_or(false);
    if same || candidate.exists() {
        Ok(dest_dir.join(unique_explorer_name(dest_dir, &name)))
    } else {
        Ok(candidate)
    }
}

/// Duplicate sources onto a real host folder with Explorer unique names.
/// `from` / `path` / `paths` are sources; optional `dest` / `to` is a directory.
/// Omitted dest duplicates beside each source. Dest under `stardust-ws-*` is rejected.
pub fn duplicate(v: &Value) -> ApiResult {
    let mut sources: Vec<String> = Vec::new();
    if let Some(p) = v.get("from").and_then(|x| x.as_str()) {
        if !p.is_empty() {
            sources.push(p.to_string());
        }
    }
    if let Some(p) = v.get("path").and_then(|x| x.as_str()) {
        if !p.is_empty() && !sources.iter().any(|s| s == p) {
            sources.push(p.to_string());
        }
    }
    if let Some(arr) = v.get("paths").and_then(|x| x.as_array()) {
        for x in arr {
            if let Some(s) = x.as_str() {
                if !s.is_empty() && !sources.iter().any(|p| p == s) {
                    sources.push(s.to_string());
                }
            }
        }
    }
    if sources.is_empty() {
        return Err(ApiError::bad("source path is required"));
    }

    let dest_arg = v
        .get("dest")
        .and_then(|x| x.as_str())
        .or_else(|| v.get("to").and_then(|x| x.as_str()))
        .map(str::trim)
        .filter(|s| !s.is_empty());

    let dest_dir_explicit: Option<PathBuf> = if let Some(d) = dest_arg {
        reject_temp_workspace_dest(Path::new(d))?;
        let p = parse_abs(d)?;
        reject_temp_workspace_dest(&p)?;
        let p = canonicalize_if_exists(p);
        reject_temp_workspace_dest(&p)?;
        if !p.is_dir() {
            return Err(ApiError::bad("destination must be an existing directory"));
        }
        Some(p)
    } else {
        None
    };

    let mut copied = Vec::new();
    for src in &sources {
        let from = resolve_duplicate_source(src)?;
        let dest_dir = if let Some(ref d) = dest_dir_explicit {
            d.clone()
        } else {
            let parent = from
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .ok_or_else(|| ApiError::bad("source has no parent folder"))?;
            reject_temp_workspace_dest(parent)?;
            parent.to_path_buf()
        };
        reject_temp_workspace_dest(&dest_dir)?;
        let to = dest_file_path(&from, &dest_dir)?;
        reject_temp_workspace_dest(&to)?;
        if to.exists() {
            return Err(ApiError::conflict("destination already exists"));
        }
        copy_recursive(&from, &to)?;
        copied.push(json!({
            "from": path_to_string(&from),
            "to": path_to_string(&to),
            "name": to.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(),
        }));
    }
    Ok(json!({ "ok": true, "copied": copied }))
}

pub fn move_path(from: &str, to: &str) -> ApiResult {
    let from = resolve_abs(from)?;
    let to = resolve_abs(to)?;
    reject_forbidden_move(&from)?;
    reject_forbidden_move(&to)?;
    require_exists(&from)?;
    if to.exists() {
        return Err(ApiError::conflict("destination already exists"));
    }
    match fs::rename(&from, &to) {
        Ok(()) => Ok(json!({ "ok": true })),
        Err(_) => {
            copy_recursive(&from, &to)?;
            trash::delete(&from).map_err(|e| ApiError::internal(e.to_string()))?;
            Ok(json!({ "ok": true }))
        }
    }
}

pub fn agent_out_dir() -> PathBuf {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".stardust-out")
}

fn cap_screen_dir() -> PathBuf {
    agent_out_dir().join("cap-screen")
}

pub(crate) fn sanitize_out_name(name: &str) -> Result<String, ApiError> {
    let t = name.trim();
    if t.is_empty() {
        return Err(ApiError::bad("name is required"));
    }
    if t.contains('/') || t.contains('\\') || t.contains(':') {
        return Err(ApiError::bad("name must be a file name, not a path"));
    }
    if t == "." || t == ".." || t.eq_ignore_ascii_case("cap-screen") {
        return Err(ApiError::bad("invalid name"));
    }
    if t.chars().any(|c| c.is_control() || c == '\0') {
        return Err(ApiError::bad("invalid name"));
    }
    Ok(t.to_string())
}

pub fn agent_out_write(v: &Value) -> ApiResult {
    let name = v.get("name").and_then(|x| x.as_str()).unwrap_or("");
    let name = sanitize_out_name(name)?;
    let content = if let Some(s) = v.get("content").and_then(|x| x.as_str()) {
        s.to_string()
    } else if let Some(other) = v.get("content") {
        other.to_string()
    } else {
        String::new()
    };
    let dir = agent_out_dir();
    fs::create_dir_all(&dir).map_err(io_err)?;
    let dest = dir.join(&name);
    if dest.parent() != Some(dir.as_path()) {
        return Err(ApiError::bad("invalid name"));
    }
    fs::write(&dest, content.as_bytes()).map_err(io_err)?;
    Ok(json!({
        "ok": true,
        "path": path_to_string(&dest),
        "name": name,
    }))
}

fn sweep_prefix_dir(dir: &Path, prefixes: &[&str]) {
    let Ok(rd) = fs::read_dir(dir) else {
        return;
    };
    for ent in rd.flatten() {
        let name = ent.file_name().to_string_lossy().to_ascii_lowercase();
        if prefixes.iter().any(|p| name.starts_with(p)) {
            let p = ent.path();
            let _ = if p.is_dir() {
                fs::remove_dir_all(&p)
            } else {
                fs::remove_file(&p)
            };
        }
    }
}

pub fn sweep_agent_caps() -> ApiResult {
    let cap = cap_screen_dir();
    let _ = fs::create_dir_all(&cap);
    if cap.is_dir() {
        if let Ok(rd) = fs::read_dir(&cap) {
            for ent in rd.flatten() {
                let p = ent.path();
                let _ = if p.is_dir() {
                    fs::remove_dir_all(&p)
                } else {
                    fs::remove_file(&p)
                };
            }
        }
    }
    for desk in extra_search_roots() {
        sweep_prefix_dir(&desk, &["cap-screen", "wa-view"]);
    }
    Ok(json!({ "ok": true }))
}

#[derive(Serialize)]
struct PrettyNode {
    name: String,
    kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    size: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    offset: Option<u64>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    children: Vec<PrettyNode>,
}

fn leaf(name: impl Into<String>, kind: impl Into<String>) -> PrettyNode {
    PrettyNode {
        name: name.into(),
        kind: kind.into(),
        size: None,
        offset: None,
        children: Vec::new(),
    }
}

fn pretty_type(name: String, ty: &Type, offset: Option<u64>) -> PrettyNode {
    match ty {
        Type::Scalar(s) => PrettyNode {
            name,
            kind: "Scalar".into(),
            size: Some(s.size()),
            offset,
            children: vec![leaf(format!("{s:?}"), "Scalar")],
        },
        Type::Link => PrettyNode {
            name,
            kind: "Link".into(),
            size: Some(8),
            offset,
            children: Vec::new(),
        },
        Type::Struct(st) => PrettyNode {
            name,
            kind: "Struct".into(),
            size: Some(st.size),
            offset,
            children: st.fields.iter().map(pretty_field).collect(),
        },
        Type::Array(a) => PrettyNode {
            name,
            kind: "Array".into(),
            size: Some(a.size),
            offset,
            children: vec![
                leaf("count", a.count.to_string()),
                pretty_type("elem".into(), &a.elem, None),
            ],
        },
        Type::Slice(s) => PrettyNode {
            name,
            kind: "Slice".into(),
            size: None,
            offset,
            children: vec![pretty_type("elem".into(), &s.elem, None)],
        },
        Type::Columnar(c) => {
            let mut children = vec![leaf("capacity", c.capacity.to_string())];
            children.extend(c.columns.iter().map(pretty_field));
            PrettyNode {
                name,
                kind: "Columnar".into(),
                size: Some(c.size),
                offset,
                children,
            }
        }
        Type::Bitpack(b) => PrettyNode {
            name,
            kind: "Bitpack".into(),
            size: Some(b.size),
            offset,
            children: b
                .fields
                .iter()
                .map(|f| PrettyNode {
                    name: f.name.clone(),
                    kind: "BitField".into(),
                    size: None,
                    offset: None,
                    children: vec![
                        leaf("width", f.width.to_string()),
                        leaf("bit_offset", f.bit_offset.to_string()),
                    ],
                })
                .collect(),
        },
    }
}

fn pretty_field(f: &Field) -> PrettyNode {
    let mut n = pretty_type(f.name.clone(), &f.ty, Some(f.offset));
    n.size = Some(f.size);
    n
}

fn param_kind(p: &Param) -> String {
    match p {
        Param::Ptr => "Ptr".into(),
        Param::Len => "Len".into(),
        Param::OwnedPtr => "OwnedPtr".into(),
        Param::Scalar(s) => format!("{s:?}"),
    }
}

fn ret_kind(r: &Ret) -> String {
    match r {
        Ret::Void => "Void".into(),
        Ret::OwnedPtr => "OwnedPtr".into(),
        Ret::Scalar(s) => format!("{s:?}"),
    }
}

fn pretty_import(name: &str, import: &Import) -> PrettyNode {
    let args = import
        .args
        .iter()
        .enumerate()
        .map(|(i, p)| leaf(format!("arg{i}"), param_kind(p)))
        .collect();
    PrettyNode {
        name: name.into(),
        kind: "Import".into(),
        size: None,
        offset: None,
        children: vec![
            leaf("symbol", import.symbol.clone()),
            leaf("lang", format!("{:?}", import.lang)),
            PrettyNode {
                name: "args".into(),
                kind: "params".into(),
                size: None,
                offset: None,
                children: args,
            },
            leaf("ret", ret_kind(&import.ret)),
        ],
    }
}

fn pretty_class(name: &str, class: &Class) -> PrettyNode {
    let mut children = vec![leaf("layout", class.layout.clone())];
    if let Some(base) = &class.base {
        children.push(leaf("base", base.clone()));
    }
    for (m, imp) in &class.methods {
        children.push(pretty_import(m, imp));
    }
    PrettyNode {
        name: name.into(),
        kind: "Class".into(),
        size: None,
        offset: None,
        children,
    }
}

fn pretty_ir(ir: &Ir) -> PrettyNode {
    let layouts = PrettyNode {
        name: "layouts".into(),
        kind: "layouts".into(),
        size: None,
        offset: None,
        children: ir
            .layouts
            .iter()
            .map(|(n, ty)| pretty_type(n.clone(), ty, None))
            .collect(),
    };
    let imports = PrettyNode {
        name: "imports".into(),
        kind: "imports".into(),
        size: None,
        offset: None,
        children: ir
            .imports
            .iter()
            .map(|(n, im)| pretty_import(n, im))
            .collect(),
    };
    let classes = PrettyNode {
        name: "classes".into(),
        kind: "classes".into(),
        size: None,
        offset: None,
        children: ir
            .classes
            .iter()
            .map(|(n, c)| pretty_class(n, c))
            .collect(),
    };
    PrettyNode {
        name: "IR".into(),
        kind: "root".into(),
        size: None,
        offset: None,
        children: vec![layouts, imports, classes],
    }
}

fn not_layout_ok() -> Value {
    json!({ "ok": false, "error": "not a StarDust layout" })
}

fn is_layout_ext(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|s| s.to_str())
            .map(|s| s.to_ascii_lowercase())
            .as_deref(),
        Some("sd") | Some("json")
    )
}

pub fn inspect(path: &str) -> ApiResult {
    let path = resolve_abs(path)?;
    require_exists(&path)?;
    if !path.is_file() || !is_layout_ext(&path) {
        return Ok(not_layout_ok());
    }
    let meta = fs::metadata(&path).map_err(io_err)?;
    if meta.len() > FILE_CAP {
        return Ok(not_layout_ok());
    }
    let ir = match Ir::from_sd_path(&path) {
        Ok(ir) => ir,
        Err(_) => match fs::read_to_string(&path) {
            Ok(s) => {
                let s = s.strip_prefix('\u{feff}').unwrap_or(&s);
                match Ir::from_json(s) {
                    Ok(ir) => ir,
                    Err(_) => return Ok(not_layout_ok()),
                }
            }
            Err(_) => return Ok(not_layout_ok()),
        },
    };
    let pretty = pretty_ir(&ir);
    let ir_text = ir
        .to_json_pretty()
        .map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(json!({
        "ok": true,
        "pretty": pretty,
        "ir": ir_text,
    }))
}

pub fn kill_pid(pid: u32) {
    #[cfg(windows)]
    {
        let mut kill = Command::new("taskkill");
        kill.args(["/PID", &pid.to_string(), "/T", "/F"])
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        fs_viz::shell::windows_no_window(&mut kill);
        let _ = kill.status();
    }
    #[cfg(not(windows))]
    {
        let _ = Command::new("kill")
            .args(["-9", &pid.to_string()])
            .status();
    }
}

pub fn cargo_test(cwd: &str) -> ApiResult {
    let cwd = resolve_abs(cwd)?;
    require_exists(&cwd)?;
    if !cwd.is_dir() {
        return Err(ApiError::bad("cwd must be a directory"));
    }
    let mut child = Command::new("cargo");
    child.arg("test").current_dir(&cwd);
    apply_gnu_linker(&mut child);
    fs_viz::shell::windows_no_window(&mut child);
    child.stdout(Stdio::piped()).stderr(Stdio::piped());
    let child = child
        .spawn()
        .map_err(|e| ApiError::internal(format!("spawn cargo: {e}")))?;
    let pid = child.id();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(child.wait_with_output());
    });
    match rx.recv_timeout(CARGO_TIMEOUT) {
        Ok(Ok(output)) => {
            let mut text = String::new();
            text.push_str(&String::from_utf8_lossy(&output.stdout));
            text.push_str(&String::from_utf8_lossy(&output.stderr));
            Ok(json!({
                "status": output.status.code().unwrap_or(-1),
                "output": text,
            }))
        }
        Ok(Err(e)) => Err(ApiError::internal(format!("cargo: {e}"))),
        Err(_) => {
            kill_pid(pid);
            Err(ApiError {
                status: 504,
                body: json!({ "error": "cargo test timed out" }),
            })
        }
    }
}

/// rustc GNU self-contained linker, relative to a rustup toolchain root.
const GNU_LINKER_REL: &str =
    "lib/rustlib/x86_64-pc-windows-gnu/bin/self-contained/x86_64-w64-mingw32-gcc.exe";

fn rustup_toolchains_dir() -> Option<PathBuf> {
    if let Some(home) = std::env::var_os("RUSTUP_HOME") {
        let p = PathBuf::from(home).join("toolchains");
        if p.is_dir() {
            return Some(p);
        }
    }
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"))?;
    let p = PathBuf::from(home).join(".rustup").join("toolchains");
    p.is_dir().then_some(p)
}

/// One directory level only. Never recurse: `is_dir` on rust-src (and junctions)
/// can stall Windows for minutes and jam `cargo test` / the shell pool.
fn gnu_linker_under_toolchains(toolchains: &Path) -> Option<PathBuf> {
    let rel = Path::new(GNU_LINKER_REL);
    let rd = fs::read_dir(toolchains).ok()?;
    for e in rd.flatten() {
        let Ok(ft) = e.file_type() else {
            continue;
        };
        if !ft.is_dir() {
            continue;
        }
        let gcc = e.path().join(rel);
        if gcc.is_file() {
            return Some(gcc);
        }
    }
    None
}

fn find_gnu_linker() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        gnu_linker_under_toolchains(&rustup_toolchains_dir()?)
    }
    #[cfg(not(windows))]
    {
        None
    }
}

fn apply_gnu_linker(cmd: &mut Command) {
    if std::env::var_os("CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER").is_some() {
        return;
    }
    if let Some(p) = find_gnu_linker() {
        cmd.env("CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER", p);
    }
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")))
}

fn debug_bin_dir(workspace: &Path) -> PathBuf {
    workspace.join("target").join("debug")
}

fn debug_fs_viz(workspace: &Path) -> PathBuf {
    if cfg!(windows) {
        debug_bin_dir(workspace).join("fs-viz.exe")
    } else {
        debug_bin_dir(workspace).join("fs-viz")
    }
}

/// Sidecar for a running/locked binary. Never reuse a leftover `*.old` that
/// another process still holds (os error 5 on remove/rename).
fn stash_dest(path: &Path, n: u32) -> PathBuf {
    let mut s = path.as_os_str().to_os_string();
    if n == 0 {
        s.push(".old");
    } else {
        s.push(format!(".old{n}"));
    }
    PathBuf::from(s)
}

fn cargo_lock_error(text: &str) -> bool {
    let t = text.to_ascii_lowercase();
    t.contains("os error 5") || t.contains("access is denied")
}

fn stash_one(path: &Path) {
    if !path.is_file() {
        return;
    }
    for n in 0..64u32 {
        let dest = stash_dest(path, n);
        if dest.exists() && fs::remove_file(&dest).is_err() {
            continue;
        }
        if fs::rename(path, &dest).is_ok() {
            return;
        }
    }
}

fn stash_path(path: &Path) {
    stash_one(path);
    if path.extension().and_then(|e| e.to_str()) == Some("exe") {
        stash_one(&path.with_extension("pdb"));
    }
}

fn stash_running_binaries(dir: &Path) {
    for name in [
        "fs-viz.exe",
        "stardust-view.exe",
        "stardust-open.exe",
        "stardust-guard.exe",
        "fs-viz",
        "stardust-view",
        "stardust-open",
        "stardust-guard",
    ] {
        stash_path(&dir.join(name));
    }
    // GNU ld writes hashed copies under deps/ first. Those stay locked if a
    // host process (or a hardlink to debug/*.exe) still holds them.
    let deps = dir.join("deps");
    let prefixes = [
        "fs_viz-",
        "stardust_guard-",
        "stardust_view-",
        "stardust_open-",
    ];
    if let Ok(rd) = fs::read_dir(&deps) {
        for ent in rd.flatten() {
            let path = ent.path();
            let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
                continue;
            };
            if !(name.ends_with(".exe") || name.ends_with(".pdb")) {
                continue;
            }
            if prefixes.iter().any(|p| name.starts_with(p)) {
                stash_one(&path);
            }
        }
    }
}

fn schedule_ui_relaunch(exe: &Path, cwd: &Path) -> Result<(), String> {
    if !exe.is_file() {
        return Err(format!("missing {}", exe.display()));
    }
    if !cwd.is_dir() {
        return Err(format!("missing cwd {}", cwd.display()));
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x00000200;
        let exe_s = exe
            .to_str()
            .ok_or_else(|| "relaunch path is not UTF-8".to_string())?;
        // Delay in a helper so this process can exit and drop :6006. Do not use
        // `cmd start`: its first quoted argument is the window title, so
        // `start "" "…\fs-viz.exe"` plus Command's quoting of `/C` leaves
        // ShellExecute with an empty file (Windows dialog path `\\`).
        let line = format!("ping -n 3 127.0.0.1 >nul & \"{exe_s}\" --desktop --replace");
        let comspec = std::env::var_os("ComSpec").unwrap_or_else(|| {
            let root = std::env::var_os("SystemRoot").unwrap_or_else(|| r"C:\Windows".into());
            Path::new(&root).join("System32").join("cmd.exe").into()
        });
        Command::new(comspec)
            .arg("/C")
            .raw_arg(&line)
            .current_dir(cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(fs_viz::shell::CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP)
            .spawn()
            .map_err(|e| format!("relaunch: {e}"))?;
    }
    #[cfg(not(windows))]
    {
        let exe_s = exe.display().to_string();
        Command::new("sh")
            .args([
                "-c",
                &format!("sleep 1; exec \"{exe_s}\" --desktop --replace"),
            ])
            .current_dir(cwd)
            .spawn()
            .map_err(|e| format!("relaunch: {e}"))?;
    }
    Ok(())
}

const REBUILD_FEATURE_NAMES: &[&str] = &["python", "llvm", "haskell", "lean"];

fn current_rebuild_features() -> Vec<String> {
    let mut out = Vec::new();
    if cfg!(feature = "python") {
        out.push("python".to_string());
    }
    if cfg!(feature = "llvm") {
        out.push("llvm".to_string());
    }
    if cfg!(feature = "haskell") {
        out.push("haskell".to_string());
    }
    if cfg!(feature = "lean") {
        out.push("lean".to_string());
    }
    out
}

fn union_rebuild_features(current: &[String], requested: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for name in REBUILD_FEATURE_NAMES {
        if current.iter().any(|f| f == name) || requested.iter().any(|f| f == name) {
            out.push((*name).to_string());
        }
    }
    out
}

fn normalize_rebuild_feature(name: &str) -> Result<String, ApiError> {
    let lower = name.to_ascii_lowercase();
    if REBUILD_FEATURE_NAMES.contains(&lower.as_str()) {
        Ok(lower)
    } else {
        Err(ApiError::bad(format!("unknown feature: {name}")))
    }
}

pub fn parse_rebuild_features(v: &Value) -> Result<Vec<String>, ApiError> {
    let current = current_rebuild_features();
    match v.get("features") {
        None | Some(Value::Null) => Ok(current),
        Some(Value::Array(arr)) => {
            if arr.is_empty() {
                return Ok(current);
            }
            let mut requested = Vec::with_capacity(arr.len());
            for item in arr {
                let raw = item
                    .as_str()
                    .ok_or_else(|| ApiError::bad("features must be an array of strings"))?;
                requested.push(normalize_rebuild_feature(raw)?);
            }
            Ok(union_rebuild_features(&current, &requested))
        }
        Some(_) => Err(ApiError::bad("features must be an array")),
    }
}

fn cargo_build_cmdline(features: &[String]) -> String {
    if features.is_empty() {
        "cargo build -p fs-viz".to_string()
    } else {
        format!("cargo build -p fs-viz --features {}", features.join(","))
    }
}

fn run_fs_viz_cargo(cwd: &Path, features: &[String]) -> Result<std::process::Output, ApiError> {
    let mut cmd = Command::new("cargo");
    cmd.args(["build", "-p", "fs-viz"]);
    if !features.is_empty() {
        cmd.args(["--features", &features.join(",")]);
    }
    cmd.current_dir(cwd);
    apply_gnu_linker(&mut cmd);
    fs_viz::shell::windows_no_window(&mut cmd);
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let child = cmd
        .spawn()
        .map_err(|e| ApiError::internal(format!("spawn cargo: {e}")))?;
    let pid = child.id();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(child.wait_with_output());
    });
    match rx.recv_timeout(CARGO_TIMEOUT) {
        Ok(Ok(output)) => Ok(output),
        Ok(Err(e)) => Err(ApiError::internal(format!("cargo: {e}"))),
        Err(_) => {
            kill_pid(pid);
            Err(ApiError {
                status: 504,
                body: json!({ "error": "cargo build timed out" }),
            })
        }
    }
}

pub fn rebuild_ui(features: &[String]) -> ApiResult {
    let cwd = workspace_root();
    if !cwd.is_dir() {
        return Err(ApiError::internal("workspace root missing"));
    }
    fs_viz::shell::bump_arm_grace();
    fs_viz::shell::begin_rebuild_hold();
    let bin_dir = debug_bin_dir(&cwd);
    stash_running_binaries(&bin_dir);
    let mut output = run_fs_viz_cargo(&cwd, features)?;
    let mut text = cargo_build_cmdline(features);
    text.push('\n');
    text.push_str(&String::from_utf8_lossy(&output.stdout));
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    if !output.status.success() && cargo_lock_error(&text) {
        #[cfg(windows)]
        {
            fs_viz::shell::close_native_windows();
            std::thread::sleep(Duration::from_millis(900));
        }
        stash_running_binaries(&bin_dir);
        text.push_str("\nretry after stash (os error 5 / access denied)\n");
        output = run_fs_viz_cargo(&cwd, features)?;
        text.push_str(&String::from_utf8_lossy(&output.stdout));
        text.push_str(&String::from_utf8_lossy(&output.stderr));
    }
    let ok = output.status.success();
    if ok {
        fs_viz::shell::bump_arm_grace();
        fs_viz::shell::begin_rebuild_hold();
        #[cfg(windows)]
        fs_viz::shell::close_native_windows();
        let exe = debug_fs_viz(&cwd);
        if let Err(e) = schedule_ui_relaunch(&exe, &cwd) {
            text.push_str("\n");
            text.push_str(&e);
            return Ok(json!({ "status": -1, "output": text, "relaunch": false }));
        }
        text.push_str("\n");
        text.push_str(&format!(
            "{} --desktop --replace\nRelaunching…",
            if cfg!(windows) {
                "target\\debug\\fs-viz.exe"
            } else {
                "target/debug/fs-viz"
            }
        ));
        std::thread::spawn(|| {
            std::thread::sleep(Duration::from_millis(800));
            std::process::exit(0);
        });
    }
    Ok(json!({
        "status": output.status.code().unwrap_or(-1),
        "output": text,
        "relaunch": ok,
    }))
}

/// Open the StarDust repo folder in the Cursor GUI (`Cursor.exe`, not `cursor.cmd`).
/// HTTP only `PostMessage`s `StarDustOpenCursor` to `StarDustHost`; the view thread
/// `ShellExecuteW`s `--classic --new-window` then the folder.
pub fn open_cursor() -> ApiResult {
    let cwd = workspace_root();
    if !cwd.is_dir() {
        return Err(ApiError::internal("workspace root missing"));
    }
    if fs_viz::shell::find_cursor_exe().is_none() {
        return Err(ApiError::bad("Cursor.exe not found"));
    }
    #[cfg(windows)]
    {
        fs_viz::shell::post_open_cursor().map_err(|e| {
            if e == "no StarDustHost" {
                ApiError {
                    status: 503,
                    body: json!({ "error": e }),
                }
            } else {
                ApiError::internal(e)
            }
        })?;
        return Ok(json!({ "ok": true }));
    }
    #[cfg(not(windows))]
    {
        fs_viz::shell::open_cursor_gui()
            .map_err(|e| ApiError::internal(format!("open Cursor: {e}")))?;
        Ok(json!({ "ok": true }))
    }
}

fn unique_link_name(used: &mut HashSet<String>, base: &str) -> String {
    let base = if base.is_empty() { "root" } else { base };
    let mut name = base.to_string();
    let mut n = 2u32;
    while used.contains(&name) {
        name = format!("{base}_{n}");
        n += 1;
    }
    used.insert(name.clone());
    name
}

fn remove_link(path: &Path) {
    let _ = fs::remove_dir(path);
    let _ = fs::remove_file(path);
}

fn link_dir(real: &Path, link: &Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        let mut cmd = Command::new("cmd");
        cmd.args([
            "/C",
            "mklink",
            "/J",
            &path_to_string(link),
            &path_to_string(real),
        ]);
        fs_viz::shell::windows_no_window(&mut cmd);
        let status = cmd.status().map_err(|e| e.to_string())?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("mklink /J failed ({status})"))
        }
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(real, link).map_err(|e| e.to_string())
    }
}

pub fn workspace(roots: &[String]) -> ApiResult {
    let mut resolved = Vec::new();
    for r in roots {
        let p = resolve_abs(r)?;
        require_exists(&p)?;
        if !p.is_dir() {
            return Err(ApiError::bad(format!(
                "workspace root is not a directory: {}",
                path_to_string(&p)
            )));
        }
        resolved.push(p);
    }

    if let Ok(guard) = WORKSPACE.lock() {
        if let Some((cwd, names, prev)) = guard.as_ref() {
            if prev == &resolved {
                let map: Vec<Value> = names
                    .iter()
                    .zip(prev.iter())
                    .map(|(name, real)| {
                        json!({
                            "name": name,
                            "real": path_to_string(real),
                        })
                    })
                    .collect();
                return Ok(json!({
                    "cwd": path_to_string(cwd),
                    "map": map,
                    "reused": true,
                }));
            }
        }
    }

    if let Ok(mut guard) = WORKSPACE.lock() {
        if let Some((old, names, _)) = guard.take() {
            for n in names {
                remove_link(&old.join(n));
            }
            let _ = fs::remove_dir(&old);
        }
    }

    let seq = WS_SEQ.fetch_add(1, Ordering::Relaxed);
    let cwd = std::env::temp_dir().join(format!(
        "stardust-ws-{}-{}",
        std::process::id(),
        seq
    ));
    fs::create_dir_all(&cwd).map_err(io_err)?;

    let mut used = HashSet::new();
    let mut map = Vec::new();
    let mut names = Vec::new();
    for real in &resolved {
        let base = real
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "root".into());
        let name = unique_link_name(&mut used, &base);
        let link = cwd.join(&name);
        if let Err(e) = link_dir(real, &link) {
            for n in &names {
                remove_link(&cwd.join(n));
            }
            let _ = fs::remove_dir(&cwd);
            return Err(ApiError::internal(e));
        }
        names.push(name.clone());
        map.push(json!({
            "name": name,
            "real": path_to_string(real),
        }));
    }

    if let Ok(mut guard) = WORKSPACE.lock() {
        *guard = Some((cwd.clone(), names, resolved));
    }

    Ok(json!({
        "cwd": path_to_string(&cwd),
        "map": map,
    }))
}

pub fn find_node() -> PathBuf {
    if let Ok(p) = std::env::var("STARDUST_NODE") {
        let pb = PathBuf::from(p.trim());
        if pb.is_file() {
            return pb;
        }
    }
    #[cfg(windows)]
    {
        let mut where_node = Command::new("where");
        where_node.arg("node");
        fs_viz::shell::windows_no_window(&mut where_node);
        if let Ok(out) = where_node.output() {
            if out.status.success() {
                if let Some(line) = String::from_utf8_lossy(&out.stdout).lines().next() {
                    let p = PathBuf::from(line.trim());
                    if p.is_file() {
                        return p;
                    }
                }
            }
        }
        let mut tmps = vec![std::env::temp_dir()];
        for key in ["TEMP", "TMP"] {
            if let Ok(v) = std::env::var(key) {
                let p = PathBuf::from(v);
                if !tmps.contains(&p) {
                    tmps.push(p);
                }
            }
        }
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            let p = PathBuf::from(local).join("Temp");
            if !tmps.contains(&p) {
                tmps.push(p);
            }
        }
        for tmp in &tmps {
            let pinned = tmp
                .join("node-portable-22")
                .join("node-v22.22.1-win-x64")
                .join("node.exe");
            if pinned.is_file() {
                return pinned;
            }
            let portable = tmp.join("node-portable-22");
            if let Ok(rd) = fs::read_dir(&portable) {
                for ent in rd.flatten() {
                    let exe = ent.path().join("node.exe");
                    if exe.is_file() {
                        return exe;
                    }
                    if let Ok(rd2) = fs::read_dir(ent.path()) {
                        for ent2 in rd2.flatten() {
                            let exe = ent2.path().join("node.exe");
                            if exe.is_file() {
                                return exe;
                            }
                        }
                    }
                }
            }
        }
        let system = PathBuf::from(r"C:\Program Files\nodejs\node.exe");
        if system.is_file() {
            return system;
        }
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            let user = PathBuf::from(local).join("Programs").join("nodejs").join("node.exe");
            if user.is_file() {
                return user;
            }
        }
    }
    #[cfg(not(windows))]
    {
        if let Ok(out) = Command::new("which").arg("node").output() {
            if out.status.success() {
                if let Some(line) = String::from_utf8_lossy(&out.stdout).lines().next() {
                    let p = PathBuf::from(line.trim());
                    if p.is_file() {
                        return p;
                    }
                }
            }
        }
    }
    PathBuf::from("node")
}

pub fn event_kind_str(kind: &EventKind) -> &'static str {
    match kind {
        EventKind::Create(_) => "create",
        EventKind::Modify(_) => "modify",
        EventKind::Remove(_) => "remove",
        _ => "other",
    }
}

pub fn start_watch(
    path: &str,
) -> Result<
    (
        RecommendedWatcher,
        mpsc::Receiver<notify::Result<notify::Event>>,
        PathBuf,
    ),
    ApiError,
> {
    let path = resolve_abs(path)?;
    require_exists(&path)?;
    let (tx, rx) = mpsc::channel();
    let mut watcher = RecommendedWatcher::new(
        move |res| {
            let _ = tx.send(res);
        },
        Config::default(),
    )
    .map_err(|e| ApiError::internal(format!("watch: {e}")))?;
    watcher
        .watch(&path, RecursiveMode::NonRecursive)
        .map_err(|e| ApiError::internal(format!("watch: {e}")))?;
    Ok((watcher, rx, path))
}

pub fn sidecar_script() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("agent").join("server.mjs")
}

pub fn sidecar_script_mtime_secs() -> u64 {
    sidecar_script()
        .metadata()
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn web_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("web")
}

pub fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (from_hex(bytes[i + 1]), from_hex(bytes[i + 2])) {
                out.push((h << 4) | l);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn from_hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn ui_op_ok(op: &str) -> bool {
    matches!(
        op,
        "look-at-view" | "navigate" | "open" | "select" | "pin" | "copy" | "search" | "jump"
    )
}

fn with_inbox<T>(f: impl FnOnce(&mut UiInbox) -> T) -> T {
    let mut slot = UI_INBOX.lock().unwrap_or_else(|e| e.into_inner());
    let inbox = slot.get_or_insert_with(|| UiInbox {
        pending: Vec::new(),
        waiters: HashMap::new(),
    });
    f(inbox)
}

/// Sidecar enqueue: wait until the web UI posts a result, or time out.
pub fn ui_enqueue(op: &str, args: Value) -> ApiResult {
    ui_enqueue_timeout(op, args, UI_CMD_TIMEOUT)
}

fn ui_enqueue_timeout(op: &str, args: Value, timeout: Duration) -> ApiResult {
    let op = op.trim();
    if !ui_op_ok(op) {
        return Err(ApiError::bad(
            "op must be look-at-view, navigate, open, select, pin, copy, search, or jump",
        ));
    }
    let id = format!("ui-{}", UI_SEQ.fetch_add(1, Ordering::Relaxed));
    let (tx, rx) = mpsc::channel();
    {
        let full = with_inbox(|inbox| {
            if inbox.pending.len() >= UI_INBOX_CAP {
                true
            } else {
                inbox.pending.push(UiCommand {
                    id: id.clone(),
                    op: op.to_string(),
                    args,
                });
                inbox.waiters.insert(id.clone(), tx);
                false
            }
        });
        if full {
            return Err(ApiError::conflict("ui inbox full"));
        }
    }
    match rx.recv_timeout(timeout) {
        Ok(v) => Ok(v),
        Err(RecvTimeoutError::Timeout) => {
            with_inbox(|inbox| {
                inbox.waiters.remove(&id);
                inbox.pending.retain(|c| c.id != id);
            });
            Ok(json!({
                "ok": false,
                "id": id,
                "op": op,
                "error": "ui timed out (page did not pick up the command)",
            }))
        }
        Err(RecvTimeoutError::Disconnected) => Err(ApiError::internal("ui command dropped")),
    }
}

/// Web UI short-poll: take one queued command; leave the rest.
pub fn ui_inbox_take() -> Value {
    with_inbox(|inbox| {
        let commands: Vec<Value> = if inbox.pending.is_empty() {
            Vec::new()
        } else {
            let c = inbox.pending.remove(0);
            vec![json!({
                "id": c.id,
                "op": c.op,
                "args": c.args,
            })]
        };
        json!({ "commands": commands })
    })
}

/// Web UI posts the executed result (including look-at-view image).
pub fn ui_post_result(v: &Value) -> ApiResult {
    let id = v
        .get("id")
        .and_then(|x| x.as_str())
        .ok_or_else(|| ApiError::bad("missing id"))?;
    if id.is_empty() {
        return Err(ApiError::bad("missing id"));
    }
    let sent = with_inbox(|inbox| {
        if let Some(tx) = inbox.waiters.remove(id) {
            let _ = tx.send(v.clone());
            true
        } else {
            false
        }
    });
    if sent {
        return Ok(json!({ "ok": true }));
    }
    Ok(json!({ "ok": true, "ignored": true }))
}

pub fn parse_query(q: &str) -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    if q.is_empty() {
        return map;
    }
    for pair in q.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        map.insert(
            percent_decode(&k.replace('+', " ")),
            percent_decode(&v.replace('+', " ")),
        );
    }
    map
}

#[cfg(test)]
mod search_tests {
    use super::*;

    #[test]
    fn stardust_query_ranks_root_folder_first() {
        let tmp = std::env::temp_dir().join(format!("sd-search-{}", std::process::id()));
        let root = tmp.join("StarDust");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(root.join("target").join("debug")).unwrap();
        fs::write(root.join("StarDust UI.md"), b"x").unwrap();
        fs::write(root.join("target").join("debug").join("stardust.exe"), b"x").unwrap();
        let v = search(root.to_str().unwrap(), "stardust").expect("search");
        let hits = v["hits"].as_array().expect("hits");
        assert!(!hits.is_empty(), "expected hits");
        assert_eq!(hits[0]["name"], "StarDust");
        assert!(
            hits.iter().all(|h| {
                let p = h["path"].as_str().unwrap_or("").replace('\\', "/").to_lowercase();
                !p.contains("/target/")
            }),
            "target/ artifacts must not appear"
        );
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn skip_descend_name_is_path_jump_not_walked() {
        let tmp = std::env::temp_dir().join(format!("sd-search-jump-{}", std::process::id()));
        let root = tmp.join("Peter");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(root.join("AppData").join("Local")).unwrap();
        fs::write(root.join("AppData").join("Local").join("secret.txt"), b"x").unwrap();
        fs::create_dir_all(root.join("target").join("debug")).unwrap();
        fs::write(root.join("target").join("debug").join("stardust.exe"), b"x").unwrap();
        fs::create_dir_all(root.join("Desktop")).unwrap();

        let v = search(root.to_str().unwrap(), "AppData").expect("search");
        let hits = v["hits"].as_array().expect("hits");
        assert!(
            hits.iter().any(|h| h["name"] == "AppData" && h["kind"] == "dir"),
            "AppData directory must be a hit"
        );
        assert_eq!(hits[0]["name"], "AppData", "matching AppData is a top path jump");
        let app_prefix = path_to_string(&root.join("AppData"))
            .replace('\\', "/")
            .to_lowercase()
            + "/";
        assert!(
            hits.iter().all(|h| {
                let p = h["path"].as_str().unwrap_or("").replace('\\', "/").to_lowercase();
                !p.starts_with(&app_prefix)
            }),
            "must not descend into AppData"
        );

        let abs = root.join("AppData");
        let v_abs = search(root.to_str().unwrap(), abs.to_str().unwrap()).expect("abs jump");
        let abs_hits = v_abs["hits"].as_array().expect("hits");
        assert_eq!(abs_hits[0]["name"], "AppData");

        let v_tgt = search(root.to_str().unwrap(), "target").expect("target");
        let tgt_hits = v_tgt["hits"].as_array().expect("hits");
        assert!(
            tgt_hits.iter().any(|h| h["name"] == "target" && h["kind"] == "dir"),
            "target directory must be a hit"
        );
        let tgt_prefix = path_to_string(&root.join("target"))
            .replace('\\', "/")
            .to_lowercase()
            + "/";
        assert!(
            tgt_hits.iter().all(|h| {
                let p = h["path"].as_str().unwrap_or("").replace('\\', "/").to_lowercase();
                !p.starts_with(&tgt_prefix)
            }),
            "must not descend into target"
        );
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn history_file_is_layout_sibling() {
        let layout = layout_file();
        let hist = history_file();
        assert_eq!(layout.parent(), hist.parent());
        assert_eq!(hist.file_name().and_then(|n| n.to_str()), Some("agent-history.json"));
    }

    static UI_TEST: Mutex<()> = Mutex::new(());

    fn drain_ui() {
        for _ in 0..UI_INBOX_CAP + 2 {
            let v = ui_inbox_take();
            let n = v["commands"].as_array().map(|a| a.len()).unwrap_or(0);
            if n == 0 {
                return;
            }
        }
    }

    fn ui_inbox_pending_len() -> usize {
        with_inbox(|inbox| inbox.pending.len())
    }

    #[test]
    fn ui_command_rejects_unknown_op() {
        let err = ui_enqueue("explode", json!({})).unwrap_err();
        assert_eq!(err.status, 400);
    }

    #[test]
    fn ui_copy_op_is_allowed() {
        let _g = UI_TEST.lock().unwrap_or_else(|e| e.into_inner());
        drain_ui();
        let v = ui_enqueue_timeout("copy", json!({ "paths": [] }), Duration::from_millis(40)).unwrap();
        assert_eq!(v["ok"], false);
        assert!(v["error"].as_str().unwrap_or("").contains("timed out"));
    }

    #[test]
    fn ui_search_and_jump_ops_are_allowed() {
        let _g = UI_TEST.lock().unwrap_or_else(|e| e.into_inner());
        drain_ui();
        let s = ui_enqueue_timeout("search", json!({ "q": "x" }), Duration::from_millis(40)).unwrap();
        assert!(s["error"].as_str().unwrap_or("").contains("timed out"));
        drain_ui();
        let j = ui_enqueue_timeout("jump", json!({ "path": "C:\\" }), Duration::from_millis(40)).unwrap();
        assert!(j["error"].as_str().unwrap_or("").contains("timed out"));
    }

    #[test]
    fn ui_inbox_empty_after_drain() {
        let _g = UI_TEST.lock().unwrap_or_else(|e| e.into_inner());
        drain_ui();
        let v = ui_inbox_take();
        assert_eq!(v["commands"].as_array().map(|a| a.len()), Some(0));
    }

    #[test]
    fn ui_inbox_enqueue_take_result() {
        let _g = UI_TEST.lock().unwrap_or_else(|e| e.into_inner());
        drain_ui();
        let h = std::thread::spawn(|| ui_enqueue("select", json!({ "names": ["a"] })));
        let mut got = false;
        for _ in 0..80 {
            let inbox = ui_inbox_take();
            let cmds = inbox["commands"].as_array().cloned().unwrap_or_default();
            if let Some(cmd) = cmds.first() {
                assert_eq!(cmd["op"], "select");
                let id = cmd["id"].as_str().expect("id");
                ui_post_result(&json!({ "ok": true, "id": id, "op": "select" })).unwrap();
                got = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(got, "command never appeared in inbox");
        let out = h.join().expect("join").expect("enqueue");
        assert_eq!(out["ok"], true);
        assert_eq!(out["op"], "select");
    }

    #[test]
    fn ui_inbox_take_one_leaves_queued() {
        let _g = UI_TEST.lock().unwrap_or_else(|e| e.into_inner());
        drain_ui();
        let h1 = std::thread::spawn(|| {
            ui_enqueue_timeout("select", json!({ "names": ["a"] }), Duration::from_secs(5))
        });
        let h2 = std::thread::spawn(|| {
            ui_enqueue_timeout("pin", json!({ "names": ["b"] }), Duration::from_secs(5))
        });
        let mut ready = false;
        for _ in 0..80 {
            if ui_inbox_pending_len() >= 2 {
                ready = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(ready, "both commands should queue before take");
        let first = ui_inbox_take();
        let cmds = first["commands"].as_array().cloned().unwrap_or_default();
        assert_eq!(cmds.len(), 1, "take must return one command");
        assert_eq!(ui_inbox_pending_len(), 1, "second command must remain queued");
        let id1 = cmds[0]["id"].as_str().expect("id");
        ui_post_result(&json!({ "ok": true, "id": id1, "op": cmds[0]["op"] })).unwrap();
        let second = ui_inbox_take();
        let cmds2 = second["commands"].as_array().cloned().unwrap_or_default();
        assert_eq!(cmds2.len(), 1);
        let id2 = cmds2[0]["id"].as_str().expect("id");
        ui_post_result(&json!({ "ok": true, "id": id2, "op": cmds2[0]["op"] })).unwrap();
        let _ = h1.join().expect("join1");
        let _ = h2.join().expect("join2");
    }

    #[test]
    fn ui_command_times_out_without_ui() {
        let _g = UI_TEST.lock().unwrap_or_else(|e| e.into_inner());
        drain_ui();
        let v = ui_enqueue_timeout("look-at-view", json!({}), Duration::from_millis(40)).unwrap();
        assert_eq!(v["ok"], false);
        assert!(v["error"].as_str().unwrap_or("").contains("timed out"));
        assert_eq!(ui_inbox_take()["commands"].as_array().map(|a| a.len()), Some(0));
    }
}

#[cfg(test)]
mod desktop_children_tests {
    use super::*;

    fn write_file(dir: &Path, name: &str, bytes: &[u8]) {
        fs::create_dir_all(dir).unwrap();
        fs::write(dir.join(name), bytes).unwrap();
    }

    #[test]
    fn overlay_public_extras_have_abs_path_user_names_win() {
        let tmp = std::env::temp_dir().join(format!("sd-desk-union-{}", std::process::id()));
        let user = tmp.join("user");
        let public = tmp.join("public");
        let _ = fs::remove_dir_all(&tmp);
        write_file(&user, "notes.txt", b"user");
        write_file(&user, "same.txt", b"user-same");
        write_file(&public, "same.txt", b"public-same");
        write_file(&public, "Google Chrome.lnk", b"lnk");

        let mut entries = collect_dir_entries(&user, false).expect("user");
        overlay_named_entries(&mut entries, &public);
        sort_entries(&mut entries);

        let by_name: HashMap<&str, &Entry> = entries.iter().map(|e| (e.name.as_str(), e)).collect();
        assert!(by_name["notes.txt"].path.is_none(), "user child has no overlay path");
        assert!(by_name["same.txt"].path.is_none(), "user name wins, no public path");
        let chrome = by_name["Google Chrome.lnk"];
        let chrome_path = chrome.path.as_deref().expect("public extra must have abs path");
        let want = path_to_string(&public.join("Google Chrome.lnk"));
        assert!(
            chrome_path
                .replace('\\', "/")
                .eq_ignore_ascii_case(&want.replace('\\', "/")),
            "chrome path {chrome_path} vs {want}"
        );

        let v = children(&path_to_string(&user)).expect("children of temp user");
        let names: Vec<&str> = v["entries"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|e| e["name"].as_str())
            .collect();
        assert!(names.contains(&"notes.txt"));
        assert!(
            !names.contains(&"Google Chrome.lnk"),
            "temp folder is not the session Desktop; no Public merge"
        );

        let _ = fs::remove_dir_all(&tmp);
    }

    #[cfg(windows)]
    #[test]
    fn session_desktop_children_union_public_with_abs_path() {
        let desk = fs_viz::shell::desktop_dir();
        if !desk.is_dir() {
            return;
        }
        let Some(public) = fs_viz::shell::public_desktop_dir() else {
            return;
        };
        if same_dir(&desk, &public) {
            return;
        }
        let v = children(&path_to_string(&desk)).expect("desktop children");
        let entries = v["entries"].as_array().expect("entries");
        let user_names: HashSet<String> = collect_dir_entries(&desk, false)
            .unwrap()
            .into_iter()
            .map(|e| entry_name_key(&e.name))
            .collect();
        let public_ents = collect_dir_entries(&public, true).unwrap_or_default();
        for pe in public_ents {
            let k = entry_name_key(&pe.name);
            let found = entries.iter().find(|e| {
                e["name"].as_str().map(entry_name_key) == Some(k.clone())
            });
            assert!(found.is_some(), "missing desktop name {}", pe.name);
            let found = found.unwrap();
            if user_names.contains(&k) {
                assert!(
                    found.get("path").and_then(|p| p.as_str()).is_none(),
                    "user name {} must not take public path",
                    pe.name
                );
            } else {
                let p = found["path"].as_str().expect("public extra path");
                let want = pe.path.as_deref().unwrap();
                assert!(
                    p.replace('\\', "/")
                        .eq_ignore_ascii_case(&want.replace('\\', "/")),
                    "{} path {p} vs {want}",
                    pe.name
                );
            }
        }
    }
}

#[cfg(test)]
mod duplicate_tests {
    use super::*;

    #[test]
    fn sanitize_out_name_rejects_paths() {
        assert!(sanitize_out_name("").is_err());
        assert!(sanitize_out_name("..").is_err());
        assert!(sanitize_out_name("../x.txt").is_err());
        assert!(sanitize_out_name("a/b.txt").is_err());
        assert!(sanitize_out_name("a\\b.txt").is_err());
        assert!(sanitize_out_name("cap-screen").is_err());
        assert_eq!(sanitize_out_name("notes.txt").unwrap(), "notes.txt");
    }

    #[test]
    fn explorer_copy_names_keep_extension() {
        assert_eq!(
            explorer_copy_candidate("Anchor - Cytus II.txt", 1),
            "Anchor - Cytus II - Copy.txt"
        );
        assert_eq!(
            explorer_copy_candidate("Anchor - Cytus II.txt", 2),
            "Anchor - Cytus II - Copy (2).txt"
        );
        assert_eq!(explorer_copy_candidate("README", 1), "README - Copy");
        assert_eq!(explorer_copy_candidate(".gitignore", 1), ".gitignore - Copy");
    }

    #[test]
    fn stardust_ws_component_is_detected() {
        let p = PathBuf::from(std::env::temp_dir()).join("stardust-ws-1-2").join("file.txt");
        assert!(is_stardust_ws_path(&p));
        assert!(!is_stardust_ws_path(&std::env::temp_dir()));
    }

    #[test]
    fn forbidden_path_detects_appdata_and_ws() {
        let appdata = PathBuf::from("C:")
            .join("Users")
            .join("x")
            .join("AppData")
            .join("Roaming")
            .join("StarDust");
        let legacy = PathBuf::from("C:")
            .join("Users")
            .join("x")
            .join("Application Data")
            .join("foo");
        let ws = PathBuf::from("tmp").join("stardust-ws-1").join("a");
        let desk = PathBuf::from("C:").join("Users").join("x").join("Desktop").join("file.txt");
        assert!(forbidden_path(&appdata));
        assert!(forbidden_path(&legacy));
        assert!(forbidden_path(&ws));
        assert!(!forbidden_path(&desk));
    }

    #[test]
    fn exists_missing_path_is_false() {
        let p = PathBuf::from(r"C:\no-such-stardust-exists-probe").join(format!(
            "missing-{}.txt",
            std::process::id()
        ));
        let v = exists(&path_to_string(&p)).expect("exists");
        assert_eq!(v["exists"], false);
        assert!(v.get("kind").is_none());
        assert!(v.get("children").is_none());
    }

    #[test]
    fn mkdir_rejects_appdata() {
        let appdata = PathBuf::from(r"C:\Users\x\AppData\Roaming\StarDust\agent-mkdir-nope");
        let err = mkdir(&path_to_string(&appdata)).expect_err("AppData mkdir");
        assert_eq!(err.status, 400);
        let msg = err.body["error"].as_str().unwrap_or("");
        assert!(
            msg.contains("AppData") || msg.contains("temp workspace"),
            "{msg}"
        );
    }

    #[test]
    fn duplicate_beside_source_uses_explorer_names() {
        let tmp = std::env::temp_dir().join(format!("sd-dup-{}", std::process::id()));
        let dir = tmp.join("desk");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&dir).unwrap();
        let src = dir.join("Anchor - Cytus II.txt");
        fs::write(&src, b"song").unwrap();
        let src_s = path_to_string(&src);

        let v1 = duplicate(&json!({ "from": src_s })).expect("first copy");
        let n1 = v1["copied"][0]["name"].as_str().unwrap();
        assert_eq!(n1, "Anchor - Cytus II - Copy.txt");
        assert_eq!(fs::read(dir.join(n1)).unwrap(), b"song");

        let v2 = duplicate(&json!({ "from": src_s })).expect("second copy");
        let n2 = v2["copied"][0]["name"].as_str().unwrap();
        assert_eq!(n2, "Anchor - Cytus II - Copy (2).txt");

        let other = tmp.join("other");
        fs::create_dir_all(&other).unwrap();
        let v3 = duplicate(&json!({
            "from": src_s,
            "dest": path_to_string(&other),
        }))
        .expect("other folder keeps name");
        assert_eq!(v3["copied"][0]["name"], "Anchor - Cytus II.txt");

        let ws = tmp.join("stardust-ws-9-1");
        fs::create_dir_all(&ws).unwrap();
        let err = duplicate(&json!({
            "from": src_s,
            "dest": path_to_string(&ws),
        }))
        .unwrap_err();
        assert_eq!(err.status, 400);
        assert!(
            err.body["error"]
                .as_str()
                .unwrap_or("")
                .contains("temp workspace"),
            "reject overlay dest: {}",
            err.body
        );

        let _ = fs::remove_dir_all(&tmp);
    }
}

#[cfg(test)]
mod gnu_linker_tests {
    use super::*;

    fn write_exe(path: &Path) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, b"not-a-real-gcc").unwrap();
    }

    #[test]
    fn gnu_linker_lookup_is_one_toolchain_level_not_recursive() {
        let tmp = std::env::temp_dir().join(format!("sd-gnu-linker-{}", std::process::id()));
        let toolchains = tmp.join("toolchains");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&toolchains).unwrap();

        let decoy = toolchains
            .join("stable-x86_64-pc-windows-gnu")
            .join("lib")
            .join("rustlib")
            .join("src")
            .join("rust")
            .join("self-contained")
            .join("x86_64-w64-mingw32-gcc.exe");
        write_exe(&decoy);

        assert!(
            super::gnu_linker_under_toolchains(&toolchains).is_none(),
            "nested rust-src decoy must not be found"
        );

        let real = toolchains
            .join("stable-x86_64-pc-windows-gnu")
            .join(super::GNU_LINKER_REL);
        write_exe(&real);
        let found = super::gnu_linker_under_toolchains(&toolchains).expect("canonical linker");
        assert_eq!(found, real);

        let _ = fs::remove_dir_all(&tmp);
    }
}

#[cfg(test)]
mod ime_tests {
    use super::*;

    #[test]
    fn parse_ime_composing_true_false_and_missing_is_400() {
        assert_eq!(parse_ime_composing(&json!({ "composing": true })).unwrap(), true);
        assert_eq!(parse_ime_composing(&json!({ "composing": false })).unwrap(), false);
        let missing = parse_ime_composing(&json!({})).unwrap_err();
        assert_eq!(missing.status, 400);
        let bad = parse_ime_composing(&json!({ "composing": "yes" })).unwrap_err();
        assert_eq!(bad.status, 400);
    }
}

#[cfg(test)]
mod parse_rebuild_tests {
    use super::*;

    fn current_set() -> Vec<String> {
        current_rebuild_features()
    }

    #[test]
    fn parse_rebuild_empty_json_is_current_set() {
        assert_eq!(parse_rebuild_features(&json!({})).unwrap(), current_set());
        assert_eq!(
            parse_rebuild_features(&json!({ "features": Value::Null })).unwrap(),
            current_set()
        );
        assert_eq!(
            parse_rebuild_features(&json!({ "features": [] })).unwrap(),
            current_set()
        );
    }

    #[test]
    fn parse_rebuild_python_includes_python() {
        let got = parse_rebuild_features(&json!({ "features": ["python"] })).unwrap();
        assert!(got.iter().any(|f| f == "python"));
    }

    #[test]
    fn parse_rebuild_unknown_evil_is_400() {
        let err = parse_rebuild_features(&json!({ "features": ["evil"] })).unwrap_err();
        assert_eq!(err.status, 400);
    }

    #[test]
    fn parse_rebuild_mixed_case_python_accepted() {
        let got = parse_rebuild_features(&json!({ "features": ["Python"] })).unwrap();
        assert!(got.iter().any(|f| f == "python"));
    }

    #[test]
    fn parse_rebuild_non_array_features_is_400() {
        let err = parse_rebuild_features(&json!({ "features": "python" })).unwrap_err();
        assert_eq!(err.status, 400);
    }
}

#[cfg(all(test, windows))]
mod cargo_spawn_stdio_tests {
    use super::*;

    #[test]
    fn no_window_then_pipe_spawns_cmd() {
        let mut cmd = Command::new("cmd");
        cmd.args(["/C", "exit", "0"]);
        fs_viz::shell::windows_no_window(&mut cmd);
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        let out = cmd.output().expect("spawn cmd");
        assert!(out.status.success(), "{out:?}");
    }
}
