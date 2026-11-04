use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=parse.c");
    println!("cargo:rerun-if-changed=parse_eth.rs");
    println!("cargo:rerun-if-env-changed=CC");

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let target = env::var("TARGET").unwrap_or_default();

    compile_c(&out_dir, &manifest_dir, &target);
    compile_rust(&out_dir, &manifest_dir, &target);
}

fn compile_c(out_dir: &std::path::Path, manifest_dir: &std::path::Path, target: &str) {
    let src = manifest_dir.join("parse.c");
    let compiler = cc::Build::new().get_compiler();
    if compiler.is_like_msvc() {
        panic!(
            "stardust-packet compiles real C with gcc/clang (-shared). \
             Detected MSVC compiler `{}` for target `{target}`. \
             MSVC / link.exe is not supported; use Windows GNU rustc with gcc.",
            compiler.path().display()
        );
    }

    let lib_name = if target.contains("windows") {
        "packet.dll"
    } else if target.contains("apple") {
        "libpacket.dylib"
    } else {
        "libpacket.so"
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
            "failed to compile parse.c to {} with `{}` (status {}):\n{}\n{}",
            dest.display(),
            compiler.path().display(),
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
    }

    println!("cargo:rustc-env=STARDUST_PACKET_C_LIB={}", dest.display());
}

fn compile_rust(out_dir: &std::path::Path, manifest_dir: &std::path::Path, target: &str) {
    let src = manifest_dir.join("parse_eth.rs");
    let rustc = env::var("RUSTC").expect("RUSTC");

    let lib_name = if target.contains("windows") {
        "packet_rs.dll"
    } else if target.contains("apple") {
        "libpacket_rs.dylib"
    } else {
        "libpacket_rs.so"
    };
    let dest = out_dir.join(lib_name);

    let mut cmd = Command::new(&rustc);
    cmd.arg("--edition").arg("2021");
    cmd.arg("--crate-type").arg("cdylib");
    cmd.arg("-C").arg("panic=abort");
    cmd.arg("--target").arg(target);
    let linker_key = format!(
        "CARGO_TARGET_{}_LINKER",
        target.replace(['-', '.'], "_").to_ascii_uppercase()
    );
    if let Ok(linker) = env::var(&linker_key) {
        cmd.arg("-C").arg(format!("linker={linker}"));
    } else if target.contains("windows") && target.contains("gnu") {
        if let Some(gcc) = self_contained_mingw_gcc(&rustc) {
            cmd.arg("-C").arg(format!("linker={}", gcc.display()));
        }
    }
    cmd.arg("-o").arg(&dest);
    cmd.arg(&src);

    let output = cmd.output().unwrap_or_else(|e| {
        panic!("failed to spawn rustc `{rustc}`: {e}");
    });
    if !output.status.success() {
        panic!(
            "failed to compile parse_eth.rs to {} with `{rustc}` (status {}):\n{}\n{}",
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

    println!("cargo:rustc-env=STARDUST_PACKET_RS_LIB={}", dest.display());
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
