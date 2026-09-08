use std::env;
use std::ffi::OsString;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::Command;

fn looks_like_msvc_link(stdout: &str, stderr: &str) -> bool {
    let combined = format!("{stdout}\n{stderr}");
    let lower = combined.to_ascii_lowercase();
    lower.contains("link.exe")
        || combined.contains("Microsoft (R) Incremental Linker")
        || combined.contains("LINK :")
}

fn tool_path() -> OsString {
    let mut dirs: Vec<PathBuf> = env::split_paths(&env::var_os("PATH").unwrap_or_default()).collect();
    let mut extras = Vec::new();
    if let Ok(h) = env::var("ELAN_HOME") {
        extras.push(PathBuf::from(h).join("bin"));
    }
    if let Some(home) = env::var_os("USERPROFILE").or_else(|| env::var_os("HOME")) {
        extras.push(PathBuf::from(home).join(".elan").join("bin"));
    }
    for extra in extras.into_iter().rev() {
        if extra.is_dir() && !dirs.iter().any(|d| d == &extra) {
            dirs.insert(0, extra);
        }
    }
    env::join_paths(dirs).expect("PATH with elan")
}

fn run_tool(name: &str, path: &OsString, configure: impl FnOnce(&mut Command)) -> std::process::Output {
    let mut cmd = Command::new(name);
    cmd.env("PATH", path);
    configure(&mut cmd);
    cmd.output().unwrap_or_else(|e| {
        if e.kind() == ErrorKind::NotFound {
            panic!(
                "{name} not found on PATH. Wave W11 compiles in-tree Lean 4 with `lean`/`leanc` \
                 (`lean -c` then `leanc -shared -fPIC`). Install Lean 4 (not Lean 3) and put \
                 `lean` and `leanc` on PATH. This fixture never skips."
            );
        }
        panic!("failed to spawn {name}: {e}");
    })
}

fn check_output(name: &str, dest: &Path, output: &std::process::Output, what: &str) {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if looks_like_msvc_link(&stdout, &stderr) {
        panic!(
            "stardust-lean: MSVC-style link detected (link.exe). \
             MSVC / link.exe is not supported; use Windows GNU rustc with Lean 4.\n\
             {name} (status {}):\n{}\n{}",
            output.status, stdout, stderr,
        );
    }

    if !output.status.success() {
        panic!(
            "failed to {what} with {name} (status {}):\n{}\n{}",
            output.status, stdout, stderr,
        );
    }

    if what.starts_with("compile") && !dest.is_file() {
        panic!(
            "{name} reported success but {} was not produced.\n{}\n{}",
            dest.display(),
            stdout,
            stderr
        );
    }
}

fn main() {
    println!("cargo:rerun-if-changed=Mutate.lean");
    println!("cargo:rerun-if-changed=mutate.c");
    println!("cargo:rerun-if-env-changed=ELAN_HOME");

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let lean_src = manifest_dir.join("Mutate.lean");
    let c_src = manifest_dir.join("mutate.c");
    let target = env::var("TARGET").unwrap_or_default();

    if target.contains("msvc") {
        panic!(
            "stardust-lean compiles Lean 4 with leanc (-shared). \
             Detected MSVC target `{target}`. \
             MSVC / link.exe is not supported; use Windows GNU rustc with Lean 4."
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
    let generated_c = out_dir.join("Mutate_gen.c");
    let lean_src_out = out_dir.join("Mutate.lean");
    let path = tool_path();
    let windows = target.contains("windows");

    fs::copy(&lean_src, &lean_src_out).unwrap_or_else(|e| {
        panic!(
            "failed to copy {} to {}: {e}",
            lean_src.display(),
            lean_src_out.display()
        )
    });

    // Mutate_gen.c (not Mutate.c): Windows is case-insensitive and would collide with mutate.c.
    let lean_c = run_tool("lean", &path, |cmd| {
        cmd.current_dir(&out_dir);
        cmd.arg("-c");
        cmd.arg(&generated_c);
        cmd.arg(&lean_src_out);
    });
    check_output("lean", &generated_c, &lean_c, "compile Mutate.lean to C");

    let leanc = run_tool("leanc", &path, |cmd| {
        cmd.current_dir(&out_dir);
        cmd.arg("-shared");
        cmd.arg("-fPIC");
        if !windows {
            cmd.arg("-Wl,-rpath,$ORIGIN");
        }
        cmd.arg("-o");
        cmd.arg(&dest);
        cmd.arg(&c_src);
        cmd.arg(&generated_c);
    });
    check_output(
        "leanc",
        &dest,
        &leanc,
        &format!("compile mutate.c + Mutate_gen.c to {}", dest.display()),
    );

    let prefix = lean_prefix(&path);
    copy_runtime_deps(&dest, &prefix, windows);

    println!("cargo:rustc-env=STARDUST_LEAN_LIB={}", dest.display());
}

fn lean_prefix(path: &OsString) -> PathBuf {
    let output = run_tool("lean", path, |cmd| {
        cmd.arg("--print-prefix");
    });
    if !output.status.success() {
        panic!(
            "lean --print-prefix failed (status {}):\n{}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
    }
    PathBuf::from(String::from_utf8_lossy(&output.stdout).trim())
}

fn search_dirs(prefix: &Path) -> Vec<PathBuf> {
    let mut dirs = vec![prefix.to_path_buf()];
    dirs.push(prefix.join("bin"));
    dirs.push(prefix.join("lib"));
    dirs.push(prefix.join("lib").join("lean"));
    if let Ok(entries) = fs::read_dir(prefix) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                dirs.push(path);
            }
        }
    }
    dirs
}

fn copy_runtime_deps(dest: &Path, prefix: &Path, windows: bool) {
    let dest_dir = match dest.parent() {
        Some(dir) => dir.to_path_buf(),
        None => return,
    };
    let dirs = search_dirs(prefix);
    if windows {
        copy_windows_deps(dest, &dest_dir, &dirs);
    } else {
        copy_unix_deps(dest, &dest_dir, &dirs);
    }
}

fn is_system_dll(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let stem = lower.split('.').next().unwrap_or(&lower);
    if lower.starts_with("api-ms-win-") || lower.starts_with("ext-ms-") {
        return true;
    }
    matches!(
        stem,
        "kernel32"
            | "kernelbase"
            | "ntdll"
            | "user32"
            | "gdi32"
            | "advapi32"
            | "shell32"
            | "ole32"
            | "oleaut32"
            | "ws2_32"
            | "wsock32"
            | "comdlg32"
            | "comctl32"
            | "imm32"
            | "setupapi"
            | "version"
            | "winmm"
            | "bcrypt"
            | "crypt32"
            | "iphlpapi"
            | "dbghelp"
            | "psapi"
            | "rpcrt4"
            | "shlwapi"
            | "ucrtbase"
            | "sechost"
            | "msvcrt"
            | "vcruntime140"
            | "vcruntime140_1"
            | "msvcp140"
            | "combase"
            | "shcore"
            | "userenv"
            | "normaliz"
            | "winhttp"
            | "wldap32"
            | "mswsock"
            | "dnsapi"
            | "bcryptprimitives"
            | "cryptbase"
            | "cryptsp"
            | "sspicli"
            | "profapi"
            | "powrprof"
            | "cfgmgr32"
            | "win32u"
            | "msasn1"
            | "ntmarta"
            | "wldp"
            | "icu"
    )
}

fn is_system_so(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.starts_with("linux-vdso")
        || lower.starts_with("ld-linux")
        || lower.starts_with("libc.so")
        || lower.starts_with("libm.so")
        || lower.starts_with("libdl.so")
        || lower.starts_with("libpthread.so")
        || lower.starts_with("librt.so")
        || lower.starts_with("libutil.so")
        || lower.starts_with("libresolv.so")
        || lower.starts_with("libnss_")
        || lower.starts_with("libgcc_s.so")
}

fn find_in_dirs(dirs: &[PathBuf], name: &str) -> Option<PathBuf> {
    for dir in dirs {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_file() {
                    continue;
                }
                if path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.eq_ignore_ascii_case(name))
                {
                    return Some(path);
                }
            }
        }
    }
    None
}

fn copy_file_to(dir: &Path, src: &Path) -> Option<PathBuf> {
    let name = src.file_name()?;
    let dest = dir.join(name);
    if dest.exists() {
        return Some(dest);
    }
    fs::copy(src, &dest)
        .unwrap_or_else(|e| panic!("failed to copy {} to {}: {e}", src.display(), dest.display()));
    Some(dest)
}

fn copy_windows_deps(root: &Path, dest_dir: &Path, dirs: &[PathBuf]) {
    let mut stack = vec![root.to_path_buf()];
    let mut seen = std::collections::BTreeSet::new();
    while let Some(current) = stack.pop() {
        let Ok(imports) = pe_imported_dlls(&current) else {
            continue;
        };
        for name in imports {
            if is_system_dll(&name) {
                continue;
            }
            let key = name.to_ascii_lowercase();
            if !seen.insert(key) {
                continue;
            }
            if let Some(src) = find_in_dirs(dirs, &name) {
                if let Some(copied) = copy_file_to(dest_dir, &src) {
                    stack.push(copied);
                }
            }
        }
    }
}

fn copy_unix_deps(root: &Path, dest_dir: &Path, dirs: &[PathBuf]) {
    let output = match Command::new("ldd").arg(root).output() {
        Ok(o) => o,
        Err(_) => return,
    };
    if !output.status.success() {
        return;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let (name, resolved) = if let Some((left, right)) = line.split_once("=>") {
            let name = left.trim();
            let right = right.trim();
            let path = right.split_whitespace().next().unwrap_or("");
            (name.to_string(), if path == "not" { None } else { Some(path.to_string()) })
        } else {
            continue;
        };
        if is_system_so(&name) {
            continue;
        }
        let src = if let Some(path) = resolved {
            let p = PathBuf::from(&path);
            if p.is_file() {
                p
            } else if let Some(found) = find_in_dirs(dirs, &name) {
                found
            } else {
                continue;
            }
        } else if let Some(found) = find_in_dirs(dirs, &name) {
            found
        } else {
            continue;
        };
        copy_file_to(dest_dir, &src);
    }
}

fn pe_imported_dlls(path: &Path) -> Result<Vec<String>, ()> {
    let data = fs::read(path).map_err(|_| ())?;
    if data.len() < 0x40 || &data[0..2] != b"MZ" {
        return Err(());
    }
    let e_lfanew = read_u32(&data, 0x3C)? as usize;
    if data.get(e_lfanew..e_lfanew + 4) != Some(&b"PE\0\0"[..]) {
        return Err(());
    }
    let coff = e_lfanew + 4;
    let num_sections = read_u16(&data, coff + 2)? as usize;
    let opt_size = read_u16(&data, coff + 16)? as usize;
    let opt = coff + 20;
    let magic = read_u16(&data, opt)?;
    let import_rva = if magic == 0x20b {
        read_u32(&data, opt + 120)?
    } else if magic == 0x10b {
        read_u32(&data, opt + 104)?
    } else {
        return Err(());
    };
    if import_rva == 0 {
        return Ok(Vec::new());
    }
    let sections_off = opt + opt_size;
    let mut sections = Vec::new();
    for i in 0..num_sections {
        let off = sections_off + i * 40;
        let va = read_u32(&data, off + 12)?;
        let vs = read_u32(&data, off + 8)?;
        let raw = read_u32(&data, off + 20)?;
        sections.push((va, vs, raw));
    }
    let mut imports = Vec::new();
    for i in 0..512 {
        let desc = match rva_to_offset(&sections, import_rva + (i as u32) * 20) {
            Some(o) => o,
            None => break,
        };
        let name_rva = match read_u32(&data, desc + 12) {
            Ok(v) => v,
            Err(_) => break,
        };
        if name_rva == 0 {
            break;
        }
        let name_off = match rva_to_offset(&sections, name_rva) {
            Some(o) => o,
            None => break,
        };
        if let Some(name) = read_cstr(&data, name_off) {
            imports.push(name);
        }
    }
    Ok(imports)
}

fn rva_to_offset(sections: &[(u32, u32, u32)], rva: u32) -> Option<usize> {
    for &(va, vs, raw) in sections {
        let size = vs.max(1);
        if rva >= va && rva < va.saturating_add(size) {
            return Some((raw + (rva - va)) as usize);
        }
    }
    None
}

fn read_u16(data: &[u8], off: usize) -> Result<u16, ()> {
    data.get(off..off + 2)
        .and_then(|s| s.try_into().ok())
        .map(u16::from_le_bytes)
        .ok_or(())
}

fn read_u32(data: &[u8], off: usize) -> Result<u32, ()> {
    data.get(off..off + 4)
        .and_then(|s| s.try_into().ok())
        .map(u32::from_le_bytes)
        .ok_or(())
}

fn read_cstr(data: &[u8], off: usize) -> Option<String> {
    let slice = data.get(off..)?;
    let end = slice.iter().position(|&b| b == 0)?;
    String::from_utf8(slice[..end].to_vec()).ok()
}
