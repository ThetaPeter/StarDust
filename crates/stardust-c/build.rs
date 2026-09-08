use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=process.c");
    println!("cargo:rerun-if-env-changed=CC");

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let src = manifest_dir.join("process.c");
    let target = env::var("TARGET").unwrap_or_default();

    let compiler = cc::Build::new().get_compiler();
    if compiler.is_like_msvc() {
        panic!(
            "stardust-c compiles real C with gcc/clang (-shared). \
             Detected MSVC compiler `{}` for target `{target}`. \
             MSVC / link.exe is not supported; use Windows GNU rustc with gcc.",
            compiler.path().display()
        );
    }

    let lib_name = if target.contains("windows") {
        "process.dll"
    } else if target.contains("apple") {
        "libprocess.dylib"
    } else {
        "libprocess.so"
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
            "failed to compile process.c to {} with `{}` (status {}):\n{}\n{}",
            dest.display(),
            compiler.path().display(),
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
    }

    println!("cargo:rustc-env=STARDUST_C_LIB={}", dest.display());
}
