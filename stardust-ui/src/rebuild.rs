//! In-app Rebuild StarDust UI (`cargo build -p stardust-ui` + relaunch).
//! Runs in `stardust-open --rebuild-ui` so slim `fs-viz` stays off windows-sys.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use serde_json::{json, Value};

use crate::shell;

const CARGO_TIMEOUT: Duration = Duration::from_secs(600);
const GNU_LINKER_REL: &str =
    "lib/rustlib/x86_64-pc-windows-gnu/bin/self-contained/x86_64-w64-mingw32-gcc.exe";
const REBUILD_FEATURE_NAMES: &[&str] = &["python", "llvm", "haskell"];

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
        "stardust-drive.exe",
        "fs-viz",
        "stardust-view",
        "stardust-open",
        "stardust-guard",
        "stardust-drive",
    ] {
        stash_path(&dir.join(name));
    }
    let deps = dir.join("deps");
    let prefixes = [
        "fs_viz-",
        "stardust_guard-",
        "stardust_view-",
        "stardust_open-",
        "stardust_drive-",
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

fn kill_pid(pid: u32) {
    #[cfg(windows)]
    {
        let mut kill = Command::new("taskkill");
        kill.args(["/PID", &pid.to_string(), "/T", "/F"])
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        shell::windows_no_window(&mut kill);
        let _ = kill.status();
    }
    #[cfg(not(windows))]
    {
        let _ = Command::new("kill").args(["-9", &pid.to_string()]).status();
    }
}

fn cargo_build_cmdline(features: &[String]) -> String {
    if features.is_empty() {
        "cargo build -p stardust-ui".to_string()
    } else {
        format!(
            "cargo build -p stardust-ui --features {}",
            features.join(",")
        )
    }
}

fn run_fs_viz_cargo(cwd: &Path, features: &[String]) -> Result<std::process::Output, String> {
    let mut cmd = Command::new("cargo");
    cmd.args(["build", "-p", "stardust-ui"]);
    if !features.is_empty() {
        cmd.args(["--features", &features.join(",")]);
    }
    cmd.current_dir(cwd);
    apply_gnu_linker(&mut cmd);
    shell::windows_no_window(&mut cmd);
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let child = cmd
        .spawn()
        .map_err(|e| format!("spawn cargo: {e}"))?;
    let pid = child.id();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(child.wait_with_output());
    });
    match rx.recv_timeout(CARGO_TIMEOUT) {
        Ok(Ok(output)) => Ok(output),
        Ok(Err(e)) => Err(format!("cargo: {e}")),
        Err(_) => {
            kill_pid(pid);
            Err("cargo build timed out".into())
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
        let view = exe.with_file_name("stardust-view.exe");
        let view_s = view.to_str().unwrap_or("");
        let line = if view.is_file() {
            format!(
                "ping -n 3 127.0.0.1 >nul & \"{exe_s}\" --desktop --replace & start \"\" \"{view_s}\" --desktop"
            )
        } else {
            format!("ping -n 3 127.0.0.1 >nul & \"{exe_s}\" --desktop --replace")
        };
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
            .creation_flags(shell::CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP)
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

fn normalize_rebuild_feature(name: &str) -> Result<String, String> {
    let lower = name.to_ascii_lowercase();
    if REBUILD_FEATURE_NAMES.contains(&lower.as_str()) {
        Ok(lower)
    } else {
        Err(format!("unknown feature: {name}"))
    }
}

pub fn parse_rebuild_features(v: &Value) -> Result<Vec<String>, String> {
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
                    .ok_or_else(|| "features must be an array of strings".to_string())?;
                requested.push(normalize_rebuild_feature(raw)?);
            }
            Ok(union_rebuild_features(&current, &requested))
        }
        Some(_) => Err("features must be an array".into()),
    }
}

pub fn rebuild_ui(features: &[String]) -> Result<Value, (u16, Value)> {
    let cwd = shell::ui_cargo_root();
    if !cwd.is_dir() {
        return Err((
            500,
            json!({ "error": "workspace root missing" }),
        ));
    }
    shell::bump_arm_grace();
    shell::begin_rebuild_hold();
    let bin_dir = debug_bin_dir(&cwd);
    stash_running_binaries(&bin_dir);
    let mut output = run_fs_viz_cargo(&cwd, features).map_err(|e| (500, json!({ "error": e })))?;
    let mut text = cargo_build_cmdline(features);
    text.push('\n');
    text.push_str(&String::from_utf8_lossy(&output.stdout));
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    if !output.status.success() && cargo_lock_error(&text) {
        #[cfg(windows)]
        {
            shell::close_native_windows();
            std::thread::sleep(Duration::from_millis(900));
        }
        stash_running_binaries(&bin_dir);
        text.push_str("\nretry after stash (os error 5 / access denied)\n");
        output = run_fs_viz_cargo(&cwd, features).map_err(|e| (500, json!({ "error": e })))?;
        text.push_str(&String::from_utf8_lossy(&output.stdout));
        text.push_str(&String::from_utf8_lossy(&output.stderr));
    }
    let ok = output.status.success();
    if ok {
        shell::bump_arm_grace();
        shell::begin_rebuild_hold();
        #[cfg(windows)]
        shell::close_native_windows();
        let exe = debug_fs_viz(&cwd);
        if let Err(e) = schedule_ui_relaunch(&exe, &cwd) {
            text.push('\n');
            text.push_str(&e);
            return Ok(json!({ "status": -1, "output": text, "relaunch": false }));
        }
        text.push('\n');
        text.push_str(&format!(
            "{} --desktop --replace\nRelaunching…",
            if cfg!(windows) {
                "target\\debug\\fs-viz.exe"
            } else {
                "target/debug/fs-viz"
            }
        ));
    }
    Ok(json!({
        "status": output.status.code().unwrap_or(-1),
        "output": text,
        "relaunch": ok,
    }))
}

fn write_rebuild_cli_out(v: &Value) {
    let Some(p) = std::env::var_os("STARDUST_REBUILD_OUT") else {
        return;
    };
    let _ = fs::write(p, serde_json::to_vec(v).unwrap_or_default());
}

/// `stardust-open --rebuild-ui` with JSON body in `STARDUST_REBUILD_IN`.
pub fn rebuild_ui_cli() -> i32 {
    let v = match std::env::var_os("STARDUST_REBUILD_IN") {
        Some(p) => {
            let text = fs::read_to_string(p).unwrap_or_else(|_| "{}".into());
            serde_json::from_str(&text).unwrap_or_else(|_| json!({}))
        }
        None => json!({}),
    };
    let features = match parse_rebuild_features(&v) {
        Ok(f) => f,
        Err(e) => {
            write_rebuild_cli_out(&json!({ "ok": false, "status": 400, "error": e }));
            return 1;
        }
    };
    match rebuild_ui(&features) {
        Ok(body) => {
            let mut out = body;
            if let Some(obj) = out.as_object_mut() {
                obj.insert("ok".into(), json!(true));
            }
            write_rebuild_cli_out(&out);
            0
        }
        Err((status, body)) => {
            let mut out = body;
            if let Some(obj) = out.as_object_mut() {
                obj.insert("ok".into(), json!(false));
                obj.insert("status".into(), json!(status));
            }
            write_rebuild_cli_out(&out);
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_rebuild_features_rejects_unknown() {
        let err = parse_rebuild_features(&json!({ "features": ["evil"] })).unwrap_err();
        assert!(err.contains("unknown"), "{err}");
    }

    #[test]
    fn parse_rebuild_features_empty_keeps_current() {
        let got = parse_rebuild_features(&json!({ "features": [] })).unwrap();
        assert_eq!(got, current_rebuild_features());
    }
}
