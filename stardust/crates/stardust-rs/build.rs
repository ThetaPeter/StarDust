use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=project.rs");

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let src = manifest_dir.join("project.rs");
    let rustc = env::var("RUSTC").expect("RUSTC");
    let target = env::var("TARGET").ok();

    let lib_name = match target.as_deref() {
        Some(t) if t.contains("windows") => "stardust_rs.dll",
        Some(t) if t.contains("apple") => "libstardust_rs.dylib",
        Some(_) => "libstardust_rs.so",
        None => {
            if cfg!(windows) {
                "stardust_rs.dll"
            } else if cfg!(target_os = "macos") {
                "libstardust_rs.dylib"
            } else {
                "libstardust_rs.so"
            }
        }
    };
    let dest = out_dir.join(lib_name);

    let mut cmd = Command::new(&rustc);
    cmd.arg("--edition").arg("2021");
    cmd.arg("--crate-type").arg("cdylib");
    cmd.arg("-C").arg("panic=abort");
    if let Some(t) = &target {
        cmd.arg("--target").arg(t);
        let linker_key = format!(
            "CARGO_TARGET_{}_LINKER",
            t.replace(['-', '.'], "_").to_ascii_uppercase()
        );
        if let Ok(linker) = env::var(&linker_key) {
            cmd.arg("-C").arg(format!("linker={linker}"));
        } else if t.contains("windows") && t.contains("gnu") {
            if let Some(gcc) = self_contained_mingw_gcc(&rustc) {
                cmd.arg("-C").arg(format!("linker={}", gcc.display()));
            }
        }
    }
    cmd.arg("-o").arg(&dest);
    cmd.arg(&src);

    let output = cmd.output().unwrap_or_else(|e| {
        panic!("failed to spawn rustc `{rustc}`: {e}");
    });
    if !output.status.success() {
        panic!(
            "failed to compile project.rs to {} with `{rustc}` (status {}):\n{}\n{}",
            dest.display(),
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
    }
    if !dest.is_file() {
        panic!(
            "rustc reported success but {} was not produced.\n{}\n{}",
            dest.display(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
    }

    println!("cargo:rustc-env=STARDUST_RS_LIB={}", dest.display());
}

fn self_contained_mingw_gcc(rustc: &str) -> Option<PathBuf> {
    let output = Command::new(rustc)
        .arg("--print")
        .arg("sysroot")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let sysroot = String::from_utf8_lossy(&output.stdout);
    let gcc = PathBuf::from(sysroot.trim())
        .join("lib")
        .join("rustlib")
        .join("x86_64-pc-windows-gnu")
        .join("bin")
        .join("self-contained")
        .join("x86_64-w64-mingw32-gcc.exe");
    gcc.is_file().then_some(gcc)
}
