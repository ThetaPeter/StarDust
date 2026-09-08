use std::env;
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

fn main() {
    println!("cargo:rerun-if-changed=Mutate.hs");
    println!("cargo:rerun-if-changed=mutate.c");

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let hs_src = manifest_dir.join("Mutate.hs");
    let c_src = manifest_dir.join("mutate.c");
    let target = env::var("TARGET").unwrap_or_default();

    if target.contains("msvc") {
        panic!(
            "stardust-hs compiles Haskell with ghc (-shared). \
             Detected MSVC target `{target}`. \
             MSVC / link.exe is not supported; use Windows GNU rustc with ghc."
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

    let mut cmd = Command::new("ghc");
    cmd.current_dir(&out_dir);
    cmd.arg("-shared");
    cmd.arg("-fPIC");
    cmd.arg("-outputdir");
    cmd.arg(&out_dir);
    if !target.contains("windows") {
        cmd.arg("-optl-Wl,-rpath,$ORIGIN");
    }
    cmd.arg("-o");
    cmd.arg(&dest);
    cmd.arg(&hs_src);
    cmd.arg(&c_src);

    let output = cmd.output().unwrap_or_else(|e| {
        if e.kind() == ErrorKind::NotFound {
            panic!(
                "ghc not found on PATH. Wave W10 compiles in-tree Haskell with ghc \
                 (`ghc -shared -fPIC`). Install GHC and put `ghc` on PATH. \
                 This fixture never skips."
            );
        }
        panic!("failed to spawn ghc: {e}");
    });

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if looks_like_msvc_link(&stdout, &stderr) {
        panic!(
            "stardust-hs: MSVC-style link detected (link.exe). \
             MSVC / link.exe is not supported; use Windows GNU rustc with ghc.\n\
             ghc (status {}):\n{}\n{}",
            output.status, stdout, stderr,
        );
    }

    if !output.status.success() {
        panic!(
            "failed to compile Mutate.hs + mutate.c to {} with ghc (status {}):\n{}\n{}",
            dest.display(),
            output.status,
            stdout,
            stderr,
        );
    }

    if !dest.is_file() {
        panic!(
            "ghc reported success but {} was not produced.\n{}\n{}",
            dest.display(),
            stdout,
            stderr
        );
    }

    let libdir = ghc_libdir();
    copy_runtime_deps(&dest, &libdir, target.contains("windows"));

    println!("cargo:rustc-env=STARDUST_HS_LIB={}", dest.display());
}

fn ghc_libdir() -> PathBuf {
    let output = Command::new("ghc")
        .arg("--print-libdir")
        .output()
        .unwrap_or_else(|e| panic!("failed to spawn ghc --print-libdir: {e}"));
    if !output.status.success() {
        panic!(
            "ghc --print-libdir failed (status {}):\n{}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
    }
    PathBuf::from(String::from_utf8_lossy(&output.stdout).trim())
}

fn search_dirs(libdir: &Path) -> Vec<PathBuf> {
    let mut dirs = vec![libdir.to_path_buf()];
    if let Some(bin) = libdir.parent().map(|p| p.join("bin")) {
        dirs.push(bin);
    }
    if let Some(mingw) = libdir.parent().map(|p| p.join("mingw").join("bin")) {
        dirs.push(mingw);
    }
    dirs.push(libdir.join("bin"));
    dirs.push(libdir.join("rts"));
    dirs.push(libdir.join("mingw").join("bin"));
    if let Ok(entries) = fs::read_dir(libdir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                dirs.push(path);
            }
        }
    }
    dirs
}

fn copy_runtime_deps(dest: &Path, libdir: &Path, windows: bool) {
    let dest_dir = match dest.parent() {
        Some(dir) => dir.to_path_buf(),
        None => return,
    };
    let dirs = search_dirs(libdir);
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
