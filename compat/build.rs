//! Forward `cargo build -p fs-viz` at the git root into `stardust-ui`.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const GNU_LINKER_REL: &str =
    "lib/rustlib/x86_64-pc-windows-gnu/bin/self-contained/x86_64-w64-mingw32-gcc.exe";

const COPY_NAMES: &[&str] = &[
    "fs-viz.exe",
    "fs-viz",
    "stardust-open.exe",
    "stardust-open",
    "stardust-guard.exe",
    "stardust-guard",
    "stardust-view.exe",
    "stardust-view",
    "WebView2Loader.dll",
];

fn main() {
    // Missing path → cargo reruns this script on every build (old Tools → Rebuild).
    println!("cargo:rerun-if-changed=compat/always-rebuild-stardust-ui");
    println!("cargo:rerun-if-changed=stardust-ui/Cargo.toml");
    println!("cargo:rerun-if-changed=stardust-ui/src/main.rs");

    let root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let ui = root.join("stardust-ui");
    if !ui.join("Cargo.toml").is_file() {
        panic!("missing {}", ui.join("Cargo.toml").display());
    }

    let mut cmd = Command::new("cargo");
    cmd.args(["build", "-p", "stardust-ui"]);
    let mut feats: Vec<&str> = Vec::new();
    if env::var_os("CARGO_FEATURE_PYTHON").is_some() {
        feats.push("python");
    }
    if env::var_os("CARGO_FEATURE_LLVM").is_some() {
        feats.push("llvm");
    }
    if env::var_os("CARGO_FEATURE_HASKELL").is_some() {
        feats.push("haskell");
    }
    if !feats.is_empty() {
        cmd.args(["--features", &feats.join(",")]);
    }
    cmd.current_dir(&ui);
    apply_gnu_linker(&mut cmd);
    let status = cmd.status().unwrap_or_else(|e| panic!("spawn cargo: {e}"));
    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }

    let src = ui.join("target").join("debug");
    let dst = root.join("target").join("debug");
    let _ = fs::create_dir_all(&dst);
    for name in COPY_NAMES {
        let from = src.join(name);
        if from.is_file() {
            let to = dst.join(name);
            if let Err(e) = fs::copy(&from, &to) {
                // Live host may hold git-root target/debug/fs-viz.exe. Tools → Rebuild stashes first.
                println!("cargo:warning=copy {} skipped ({e})", name);
            }
        }
    }
}

fn apply_gnu_linker(cmd: &mut Command) {
    if env::var_os("CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER").is_some() {
        return;
    }
    if let Some(p) = find_gnu_linker() {
        cmd.env("CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER", p);
    }
}

fn find_gnu_linker() -> Option<PathBuf> {
    let toolchains = rustup_toolchains_dir()?;
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

fn rustup_toolchains_dir() -> Option<PathBuf> {
    if let Some(home) = env::var_os("RUSTUP_HOME") {
        let p = PathBuf::from(home).join("toolchains");
        if p.is_dir() {
            return Some(p);
        }
    }
    let home = env::var_os("USERPROFILE").or_else(|| env::var_os("HOME"))?;
    let p = PathBuf::from(home).join(".rustup").join("toolchains");
    p.is_dir().then_some(p)
}
