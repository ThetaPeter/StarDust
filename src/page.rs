use crate::error::Result;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Advice {
    WillNeed,
    Sequential,
    Random,
    DontNeed,
}

pub(crate) fn pin(ptr: *mut u8, size: usize) -> Result<()> {
    os::pin(ptr, size)
}

pub(crate) fn advise(ptr: *mut u8, size: usize, advice: Advice) -> Result<()> {
    os::advise(ptr, size, advice)
}

pub(crate) fn request_huge(ptr: *mut u8, size: usize) -> Result<()> {
    os::request_huge(ptr, size)
}

pub(crate) fn bind_numa(ptr: *mut u8, size: usize, node: u32) -> Result<()> {
    os::bind_numa(ptr, size, node)
}

#[cfg(windows)]
mod os {
    use super::Advice;
    use crate::error::{Error, Result};
    use windows_sys::Win32::System::Memory::VirtualLock;

    pub(super) fn pin(ptr: *mut u8, size: usize) -> Result<()> {
        let ok = unsafe { VirtualLock(ptr.cast(), size) };
        if ok == 0 {
            Err(Error::PinFailed(format!(
                "VirtualLock: {}",
                std::io::Error::last_os_error()
            )))
        } else {
            Ok(())
        }
    }

    pub(super) fn advise(_ptr: *mut u8, _size: usize, _advice: Advice) -> Result<()> {
        Err(Error::Unsupported { family: "advice" })
    }

    pub(super) fn request_huge(_ptr: *mut u8, _size: usize) -> Result<()> {
        Err(Error::Unsupported { family: "huge" })
    }

    pub(super) fn bind_numa(_ptr: *mut u8, _size: usize, _node: u32) -> Result<()> {
        Err(Error::Unsupported { family: "numa" })
    }
}

#[cfg(target_os = "linux")]
mod os {
    use super::Advice;
    use crate::error::{Error, Result};

    fn page_err(op: &str) -> Error {
        Error::PageControl(format!("{op}: {}", std::io::Error::last_os_error()))
    }

    fn madv(advice: Advice) -> libc::c_int {
        match advice {
            Advice::WillNeed => libc::MADV_WILLNEED,
            Advice::Sequential => libc::MADV_SEQUENTIAL,
            Advice::Random => libc::MADV_RANDOM,
            Advice::DontNeed => libc::MADV_DONTNEED,
        }
    }

    pub(super) fn pin(ptr: *mut u8, size: usize) -> Result<()> {
        let rc = unsafe { libc::mlock(ptr.cast(), size) };
        if rc == 0 {
            Ok(())
        } else {
            Err(Error::PinFailed(format!(
                "mlock: {}",
                std::io::Error::last_os_error()
            )))
        }
    }

    pub(super) fn advise(ptr: *mut u8, size: usize, advice: Advice) -> Result<()> {
        let rc = unsafe { libc::madvise(ptr.cast(), size, madv(advice)) };
        if rc == 0 {
            Ok(())
        } else {
            Err(page_err("madvise"))
        }
    }

    pub(super) fn request_huge(ptr: *mut u8, size: usize) -> Result<()> {
        let rc = unsafe { libc::madvise(ptr.cast(), size, libc::MADV_HUGEPAGE) };
        if rc == 0 {
            Ok(())
        } else {
            Err(page_err("madvise MADV_HUGEPAGE"))
        }
    }

    pub(super) fn bind_numa(ptr: *mut u8, size: usize, node: u32) -> Result<()> {
        let bits = libc::c_ulong::BITS;
        let idx = (node / bits) as usize;
        let shift = node % bits;
        let mut mask = vec![0 as libc::c_ulong; idx + 1];
        mask[idx] |= (1 as libc::c_ulong) << shift;
        let maxnode = libc::c_ulong::from(node) + 1;
        let rc = unsafe {
            libc::syscall(
                libc::SYS_mbind,
                ptr,
                size,
                libc::MPOL_BIND,
                mask.as_ptr(),
                maxnode,
                0u32,
            )
        };
        if rc == 0 {
            Ok(())
        } else {
            Err(Error::PageControl(format!(
                "mbind: rc={rc} {}",
                std::io::Error::last_os_error()
            )))
        }
    }
}

#[cfg(all(unix, not(target_os = "linux")))]
mod os {
    use super::Advice;
    use crate::error::{Error, Result};

    fn page_err(op: &str) -> Error {
        Error::PageControl(format!("{op}: {}", std::io::Error::last_os_error()))
    }

    fn madv(advice: Advice) -> libc::c_int {
        match advice {
            Advice::WillNeed => libc::MADV_WILLNEED,
            Advice::Sequential => libc::MADV_SEQUENTIAL,
            Advice::Random => libc::MADV_RANDOM,
            Advice::DontNeed => libc::MADV_DONTNEED,
        }
    }

    pub(super) fn pin(ptr: *mut u8, size: usize) -> Result<()> {
        let rc = unsafe { libc::mlock(ptr.cast(), size) };
        if rc == 0 {
            Ok(())
        } else {
            Err(Error::PinFailed(format!(
                "mlock: {}",
                std::io::Error::last_os_error()
            )))
        }
    }

    pub(super) fn advise(ptr: *mut u8, size: usize, advice: Advice) -> Result<()> {
        let rc = unsafe { libc::madvise(ptr.cast(), size, madv(advice)) };
        if rc == 0 {
            Ok(())
        } else {
            Err(page_err("madvise"))
        }
    }

    pub(super) fn request_huge(_ptr: *mut u8, _size: usize) -> Result<()> {
        Err(Error::Unsupported { family: "huge" })
    }

    pub(super) fn bind_numa(_ptr: *mut u8, _size: usize, _node: u32) -> Result<()> {
        Err(Error::Unsupported { family: "numa" })
    }
}

#[cfg(not(any(windows, unix)))]
compile_error!("StarDust page control requires Windows or Unix");
