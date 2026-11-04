use std::env;
use std::io::ErrorKind;
use std::path::PathBuf;
use std::process::Command;

fn looks_like_msvc_link(stdout: &str, stderr: &str) -> bool {
    let combined = format!("{stdout}\n{stderr}");
    let lower = combined.to_ascii_lowercase();
    lower.contains("link.exe")
        || combined.contains("Microsoft (R) Incremental Linker")
        || combined.contains("LINK :")
}

fn main() {
    println!("cargo:rerun-if-changed=mutate.ll");

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let src = manifest_dir.join("mutate.ll");
    let target = env::var("TARGET").unwrap_or_default();

    if target.contains("msvc") {
        panic!(
            "stardust-ll compiles LLVM IR with clang (-shared). \
             Detected MSVC target `{target}`. \
             MSVC / link.exe is not supported; use Windows GNU rustc with clang."
        );
    }

    let lib_name = if target.contains("windows") {
        "mutate.dll"
    } else if target.contains("apple") {
        "libmutate.dylib"
    } else {
        "libmutate.so"
    };
    let dest = out_dir.join(lib_name);

    let mut cmd = Command::new("clang");
    cmd.arg("-shared");
    cmd.arg("-fPIC");
    cmd.arg("-o");
    cmd.arg(&dest);
    cmd.arg(&src);

    let output = cmd.output().unwrap_or_else(|e| {
        if e.kind() == ErrorKind::NotFound {
            panic!(
                "clang not found on PATH. Wave W8 compiles in-tree LLVM IR with clang \
                 (`clang -shared -fPIC`). Install LLVM/clang and put `clang` on PATH. \
                 This fixture never skips."
            );
        }
        panic!("failed to spawn clang: {e}");
    });

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if looks_like_msvc_link(&stdout, &stderr) {
        panic!(
            "stardust-ll: MSVC-style link detected (link.exe). \
             MSVC / link.exe is not supported; use Windows GNU rustc with clang.\n\
             clang (status {}):\n{}\n{}",
            output.status, stdout, stderr,
        );
    }

    if !output.status.success() {
        panic!(
            "failed to compile mutate.ll to {} with clang (status {}):\n{}\n{}",
            dest.display(),
            output.status,
            stdout,
            stderr,
        );
    }

    println!("cargo:rustc-env=STARDUST_LL_LIB={}", dest.display());
}
