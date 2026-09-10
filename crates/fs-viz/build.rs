use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=fs.c");
    println!("cargo:rerun-if-env-changed=CC");

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let src = manifest_dir.join("fs.c");
    let target = env::var("TARGET").unwrap_or_default();

    let compiler = cc::Build::new().get_compiler();
    if compiler.is_like_msvc() {
        panic!(
            "fs-viz compiles real C with gcc/clang (-shared). \
             Detected MSVC compiler `{}` for target `{target}`. \
             MSVC / link.exe is not supported; use Windows GNU rustc with gcc.",
            compiler.path().display()
        );
    }

    let lib_name = if target.contains("windows") {
        "fs.dll"
    } else if target.contains("apple") {
        "libfs.dylib"
    } else {
        "libfs.so"
    };
    let dest = out_dir.join(lib_name);

    let mut cmd: Command = compiler.to_command();
    cmd.arg("-shared");
    cmd.arg("-fPIC");
    cmd.arg("-o");
    cmd.arg(&dest);
    cmd.arg(&src);

    let output = cmd.output().unwrap_or_else(|e| {
        panic!(
            "failed to spawn C compiler `{}`: {e}",
            compiler.path().display()
        )
    });
    if !output.status.success() {
        panic!(
            "failed to compile fs.c to {} with `{}` (status {}):\n{}\n{}",
            dest.display(),
            compiler.path().display(),
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
    }

    println!("cargo:rustc-env=STARDUST_FS_LIB={}", dest.display());

    // Committed stardust.ico is derived: knockout paper, black ink → #FFFFFF,
    // transparent (no plate). luna-silhouette.png stays black artwork.
    // Windows UI caption: stardust-caption.ico (same knockout, ink stays black).
    // Regen: crates/fs-viz/assets/gen_stardust_ico.py (Pillow).
    let ico = manifest_dir.join("assets").join("stardust.ico");
    let caption_ico = manifest_dir.join("assets").join("stardust-caption.ico");
    let png = manifest_dir
        .join("..")
        .join("..")
        .join("luna-silhouette.png");
    println!("cargo:rerun-if-changed={}", ico.display());
    println!("cargo:rerun-if-changed={}", caption_ico.display());
    println!("cargo:rerun-if-changed={}", png.display());

    if target.contains("windows") {
        let loader_dir = manifest_dir.join("webview2-loader");
        let loader = loader_dir.join("WebView2Loader.dll");
        println!("cargo:rerun-if-changed={}", loader.display());
        let mut exe_dir = out_dir.clone();
        for _ in 0..3 {
            exe_dir.pop();
        }
        if loader.is_file() {
            let _ = std::fs::copy(&loader, exe_dir.join("WebView2Loader.dll"));
            let _ = std::fs::copy(&loader, exe_dir.join("deps").join("WebView2Loader.dll"));
        }
        if ico.is_file() {
            let _ = std::fs::copy(&ico, exe_dir.join("stardust.ico"));
            let _ = std::fs::copy(&ico, exe_dir.join("deps").join("stardust.ico"));
            embed_pe_icon(&ico, &out_dir);
        } else {
            panic!(
                "missing Windows icon {} — needed as PE RT_GROUP_ICON for the taskbar",
                ico.display()
            );
        }
        if caption_ico.is_file() {
            let _ = std::fs::copy(&caption_ico, exe_dir.join("stardust-caption.ico"));
            let _ = std::fs::copy(&caption_ico, exe_dir.join("deps").join("stardust-caption.ico"));
        } else {
            panic!(
                "missing Windows caption icon {} — regen with gen_stardust_ico.py",
                caption_ico.display()
            );
        }
    }
}

/// Win11 taskbar uses the exe's PE icon, not only WM_SETICON. GNU: llvm-mingw windres.
fn embed_pe_icon(ico: &std::path::Path, out_dir: &PathBuf) {
    let rc_path = out_dir.join("stardust_icon.rc");
    let obj_path = out_dir.join("stardust_icon.o");
    let ico_posix = ico.display().to_string().replace('\\', "/");
    std::fs::write(&rc_path, format!("1 ICON \"{ico_posix}\"\n")).unwrap_or_else(|e| {
        panic!("write {}: {e}", rc_path.display());
    });

    let windres = find_windres();
    let output = Command::new(&windres)
        .arg("-i")
        .arg(&rc_path)
        .arg("-o")
        .arg(&obj_path)
        .arg("--output-format=coff")
        .output()
        .unwrap_or_else(|e| panic!("failed to spawn windres `{}`: {e}", windres.display()));
    if !output.status.success() || !obj_path.is_file() {
        panic!(
            "windres `{}` failed (status {:?}):\n{}\n{}",
            windres.display(),
            output.status.code(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
    }
    // Forward slashes: a `target\` path would inject a tab into gcc's argv.
    let obj = obj_path.display().to_string().replace('\\', "/");
    println!("cargo:rustc-link-arg-bins={obj}");
}

fn find_windres() -> PathBuf {
    println!("cargo:rerun-if-env-changed=WINDRES");
    if let Ok(explicit) = env::var("WINDRES") {
        let p = PathBuf::from(&explicit);
        if p.is_file() {
            return p;
        }
    }
    let compiler = cc::Build::new().get_compiler();
    let cc_path = compiler.path();
    if let Some(dir) = cc_path.parent() {
        for name in [
            "windres.exe",
            "windres",
            "llvm-windres.exe",
            "llvm-windres",
            "x86_64-w64-mingw32-windres.exe",
            "x86_64-w64-mingw32-windres",
        ] {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return candidate;
            }
        }
        if let Some(stem) = cc_path.file_stem().and_then(|s| s.to_str()) {
            if let Some(prefix) = stem
                .strip_suffix("-gcc")
                .or_else(|| stem.strip_suffix("-clang"))
            {
                for ext in [".exe", ""] {
                    let candidate = dir.join(format!("{prefix}-windres{ext}"));
                    if candidate.is_file() {
                        return candidate;
                    }
                }
            }
        }
    }
    PathBuf::from("windres")
}
