use std::alloc::{alloc, dealloc, Layout};
use std::fs::{File, OpenOptions};
use std::ptr::NonNull;
use std::slice;

use memmap2::MmapMut;

use crate::error::{Error, Result};

enum Backing {
    Heap {
        ptr: NonNull<u8>,
        layout: Layout,
    },
    Anon(MmapMut),
    File {
        map: MmapMut,
        _file: File,
    },
    #[cfg(target_os = "linux")]
    Module {
        map: MmapMut,
        _file: File,
    },
    #[cfg(windows)]
    Module {
        ptr: NonNull<u8>,
        _file: File,
    },
}

pub struct Region {
    name: String,
    size: usize,
    align: usize,
    backing: Backing,
}

impl Region {
    pub fn allocate(name: impl Into<String>, size: usize, align: usize) -> Result<Self> {
        if size == 0 {
            return Err(Error::Alloc);
        }
        let layout = Layout::from_size_align(size, align).map_err(|_| Error::BadAlign)?;
        let raw = unsafe { alloc(layout) };
        if raw.is_null() {
            return Err(Error::Alloc);
        }
        unsafe {
            raw.write_bytes(0, layout.size());
        }
        Ok(Self {
            name: name.into(),
            size,
            align,
            backing: Backing::Heap {
                ptr: unsafe { NonNull::new_unchecked(raw) },
                layout,
            },
        })
    }

    pub fn map_anon(name: impl Into<String>, size: usize, align: usize) -> Result<Self> {
        check_size_align(size, align)?;
        let map = MmapMut::map_anon(size).map_err(|e| Error::MapFailed(e.to_string()))?;
        from_mmap(name.into(), size, align, map, None)
    }

    pub fn map_file(
        name: impl Into<String>,
        path: impl AsRef<std::path::Path>,
        size: usize,
        align: usize,
    ) -> Result<Self> {
        check_size_align(size, align)?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(path)
            .map_err(|e| Error::MapFailed(e.to_string()))?;
        file.set_len(size as u64)
            .map_err(|e| Error::MapFailed(e.to_string()))?;
        let map =
            unsafe { MmapMut::map_mut(&file) }.map_err(|e| Error::MapFailed(e.to_string()))?;
        from_mmap(name.into(), size, align, map, Some(file))
    }

    pub fn open_file(
        name: impl Into<String>,
        path: impl AsRef<std::path::Path>,
        align: usize,
    ) -> Result<Self> {
        if !align.is_power_of_two() {
            return Err(Error::BadAlign);
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .map_err(|e| Error::MapFailed(e.to_string()))?;
        let size = file
            .metadata()
            .map_err(|e| Error::MapFailed(e.to_string()))?
            .len();
        let size = usize::try_from(size).map_err(|e| Error::MapFailed(e.to_string()))?;
        if size == 0 {
            return Err(Error::Alloc);
        }
        let map =
            unsafe { MmapMut::map_mut(&file) }.map_err(|e| Error::MapFailed(e.to_string()))?;
        from_mmap(name.into(), size, align, map, Some(file))
    }

    /// Map a named region backed by the `stardust` kernel module (`/dev/stardust`
    /// on Linux, `\\.\stardust` on Windows).
    ///
    /// Opens the device, issues `CREATE`, then maps the same handle (Linux `mmap`,
    /// Windows `MAP` ioctl). CREATE-only; to join an existing name use
    /// [`attach_module`](Self::attach_module). Last close among all holders frees
    /// the kernel name and pages. Missing device, access denied, ioctl, or map
    /// failure is [`Error::MapFailed`].
    pub fn map_module(name: impl Into<String>, size: usize, align: usize) -> Result<Self> {
        check_size_align(size, align)?;
        if align > os_page_size() {
            return Err(Error::BadAlign);
        }
        map_module_os(name.into(), size, align)
    }

    /// Attach a named region already created on the `stardust` kernel module
    /// (`/dev/stardust` on Linux, `\\.\stardust` on Windows).
    ///
    /// Opens the device and issues `ATTACH` (name in; size and align come from
    /// the creator). Then maps the same handle (Linux `mmap` of the returned size,
    /// Windows `MAP` ioctl). [`map_module`](Self::map_module) stays CREATE-only.
    /// Last close among all holders frees the kernel name and pages.
    /// Missing device, unknown name, access denied, ioctl, or map failure is
    /// [`Error::MapFailed`]. Never silent [`Ok`].
    pub fn attach_module(name: impl Into<String>) -> Result<Self> {
        attach_module_os(name.into())
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn size(&self) -> usize {
        self.size
    }

    pub fn align(&self) -> usize {
        self.align
    }

    pub fn as_ptr(&self) -> *const u8 {
        match &self.backing {
            Backing::Heap { ptr, .. } => ptr.as_ptr(),
            Backing::Anon(map) => map.as_ptr(),
            Backing::File { map, .. } => map.as_ptr(),
            #[cfg(target_os = "linux")]
            Backing::Module { map, .. } => map.as_ptr(),
            #[cfg(windows)]
            Backing::Module { ptr, .. } => ptr.as_ptr(),
        }
    }

    pub fn as_mut_ptr(&mut self) -> *mut u8 {
        match &mut self.backing {
            Backing::Heap { ptr, .. } => ptr.as_ptr(),
            Backing::Anon(map) => map.as_mut_ptr(),
            Backing::File { map, .. } => map.as_mut_ptr(),
            #[cfg(target_os = "linux")]
            Backing::Module { map, .. } => map.as_mut_ptr(),
            #[cfg(windows)]
            Backing::Module { ptr, .. } => ptr.as_ptr(),
        }
    }

    pub fn as_bytes(&self) -> &[u8] {
        unsafe { slice::from_raw_parts(self.as_ptr(), self.size) }
    }

    pub fn as_bytes_mut(&mut self) -> &mut [u8] {
        let size = self.size;
        unsafe { slice::from_raw_parts_mut(self.as_mut_ptr(), size) }
    }

    pub fn pin(&self) -> Result<()> {
        crate::page::pin(self.as_ptr().cast_mut(), self.size)
    }

    pub fn advise(&self, advice: crate::page::Advice) -> Result<()> {
        crate::page::advise(self.as_ptr().cast_mut(), self.size, advice)
    }

    pub fn request_huge(&self) -> Result<()> {
        crate::page::request_huge(self.as_ptr().cast_mut(), self.size)
    }

    pub fn bind_numa(&self, node: u32) -> Result<()> {
        crate::page::bind_numa(self.as_ptr().cast_mut(), self.size, node)
    }
}

const MODULE_NAME_LEN: usize = 64;

fn check_size_align(size: usize, align: usize) -> Result<()> {
    if size == 0 {
        return Err(Error::Alloc);
    }
    if !align.is_power_of_two() {
        return Err(Error::BadAlign);
    }
    Ok(())
}

fn encode_module_name(name: &str) -> Result<[u8; MODULE_NAME_LEN]> {
    let bytes = name.as_bytes();
    if bytes.is_empty() {
        return Err(Error::MapFailed("empty region name".into()));
    }
    if bytes.len() >= MODULE_NAME_LEN {
        return Err(Error::MapFailed("region name too long".into()));
    }
    let mut out = [0u8; MODULE_NAME_LEN];
    out[..bytes.len()].copy_from_slice(bytes);
    Ok(out)
}

fn attach_size_align(size: u64, align: u64) -> Result<(usize, usize)> {
    let size =
        usize::try_from(size).map_err(|_| Error::MapFailed("ATTACH size too large".into()))?;
    let align =
        usize::try_from(align).map_err(|_| Error::MapFailed("ATTACH align too large".into()))?;
    if size == 0 {
        return Err(Error::MapFailed("ATTACH returned size 0".into()));
    }
    if !align.is_power_of_two() {
        return Err(Error::BadAlign);
    }
    Ok((size, align))
}

fn from_mmap(
    name: String,
    size: usize,
    align: usize,
    map: MmapMut,
    file: Option<File>,
) -> Result<Region> {
    let ptr = map.as_ptr();
    if (ptr as usize) % align != 0 {
        return Err(Error::BadAlign);
    }
    let backing = match file {
        None => Backing::Anon(map),
        Some(file) => Backing::File { map, _file: file },
    };
    Ok(Region {
        name,
        size,
        align,
        backing,
    })
}

fn os_page_size() -> usize {
    #[cfg(unix)]
    {
        let n = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
        if n > 0 {
            n as usize
        } else {
            4096
        }
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::SystemInformation::{GetSystemInfo, SYSTEM_INFO};
        let mut info = unsafe { std::mem::zeroed::<SYSTEM_INFO>() };
        unsafe { GetSystemInfo(&mut info) };
        info.dwPageSize as usize
    }
}

#[cfg(target_os = "linux")]
fn map_module_os(name: String, size: usize, align: usize) -> Result<Region> {
    use std::os::unix::io::AsRawFd;

    use memmap2::MmapOptions;

    #[repr(C)]
    struct StardustCreateArg {
        name: [u8; MODULE_NAME_LEN],
        size: u64,
        align: u64,
    }

    let encoded = encode_module_name(&name)?;

    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/stardust")
        .map_err(|e| Error::MapFailed(e.to_string()))?;

    let mut arg = StardustCreateArg {
        name: encoded,
        size: size as u64,
        align: align as u64,
    };

    let req = libc::_IOW::<StardustCreateArg>(b'S', 1);
    let rc = unsafe { libc::ioctl(file.as_raw_fd(), req as _, &mut arg) };
    if rc < 0 {
        return Err(Error::MapFailed(format!(
            "CREATE ioctl: {}",
            std::io::Error::last_os_error()
        )));
    }

    let map = unsafe { MmapOptions::new().len(size).map_mut(&file) }
        .map_err(|e| Error::MapFailed(e.to_string()))?;
    let ptr = map.as_ptr();
    if (ptr as usize) % align != 0 {
        return Err(Error::BadAlign);
    }
    Ok(Region {
        name,
        size,
        align,
        backing: Backing::Module { map, _file: file },
    })
}

#[cfg(target_os = "linux")]
fn attach_module_os(name: String) -> Result<Region> {
    use std::os::unix::io::AsRawFd;

    use memmap2::MmapOptions;

    #[repr(C)]
    struct StardustCreateArg {
        name: [u8; MODULE_NAME_LEN],
        size: u64,
        align: u64,
    }

    let encoded = encode_module_name(&name)?;

    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/stardust")
        .map_err(|e| Error::MapFailed(e.to_string()))?;

    let mut arg = StardustCreateArg {
        name: encoded,
        size: 0,
        align: 0,
    };

    let req = libc::_IOWR::<StardustCreateArg>(b'S', 2);
    let rc = unsafe { libc::ioctl(file.as_raw_fd(), req as _, &mut arg) };
    if rc < 0 {
        return Err(Error::MapFailed(format!(
            "ATTACH ioctl: {}",
            std::io::Error::last_os_error()
        )));
    }

    let (size, align) = attach_size_align(arg.size, arg.align)?;
    let map = unsafe { MmapOptions::new().len(size).map_mut(&file) }
        .map_err(|e| Error::MapFailed(e.to_string()))?;
    let ptr = map.as_ptr();
    if (ptr as usize) % align != 0 {
        return Err(Error::BadAlign);
    }
    Ok(Region {
        name,
        size,
        align,
        backing: Backing::Module { map, _file: file },
    })
}

#[cfg(windows)]
fn map_module_os(name: String, size: usize, align: usize) -> Result<Region> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle, RawHandle};

    use windows_sys::Win32::Foundation::{HANDLE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem::CreateFileW;
    use windows_sys::Win32::System::IO::DeviceIoControl;

    const NAME_LEN: usize = 64;
    const FILE_DEVICE_UNKNOWN: u32 = 0x0000_0022;
    const METHOD_BUFFERED: u32 = 0;
    const FILE_ANY_ACCESS: u32 = 0;
    const IOCTL_CREATE: u32 =
        (FILE_DEVICE_UNKNOWN << 16) | (FILE_ANY_ACCESS << 14) | (0x800 << 2) | METHOD_BUFFERED;
    const IOCTL_MAP: u32 =
        (FILE_DEVICE_UNKNOWN << 16) | (FILE_ANY_ACCESS << 14) | (0x801 << 2) | METHOD_BUFFERED;
    const GENERIC_READ: u32 = 0x8000_0000;
    const GENERIC_WRITE: u32 = 0x4000_0000;
    const FILE_SHARE_READ: u32 = 0x0000_0001;
    const FILE_SHARE_WRITE: u32 = 0x0000_0002;
    const OPEN_EXISTING: u32 = 3;
    const FILE_ATTRIBUTE_NORMAL: u32 = 0x0000_0080;

    #[repr(C)]
    struct StardustCreateArg {
        name: [u8; NAME_LEN],
        size: u64,
        align: u64,
    }

    #[repr(C)]
    struct StardustMapOut {
        user_ptr: u64,
        size: u64,
    }

    let bytes = name.as_bytes();
    if bytes.is_empty() {
        return Err(Error::MapFailed("empty region name".into()));
    }
    if bytes.len() >= NAME_LEN {
        return Err(Error::MapFailed("region name too long".into()));
    }

    let path: Vec<u16> = OsStr::new(r"\\.\stardust")
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let handle = unsafe {
        CreateFileW(
            path.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            std::ptr::null_mut::<core::ffi::c_void>(),
        )
    };
    if handle == INVALID_HANDLE_VALUE || handle.is_null() {
        return Err(Error::MapFailed(format!(
            r"CreateFile \\.\stardust: {}",
            std::io::Error::last_os_error()
        )));
    }
    let file = File::from(unsafe { OwnedHandle::from_raw_handle(handle as RawHandle) });
    let h = file.as_raw_handle() as HANDLE;

    let mut arg = StardustCreateArg {
        name: [0; NAME_LEN],
        size: size as u64,
        align: align as u64,
    };
    arg.name[..bytes.len()].copy_from_slice(bytes);

    let mut returned: u32 = 0;
    let ok = unsafe {
        DeviceIoControl(
            h,
            IOCTL_CREATE,
            &arg as *const StardustCreateArg as *const core::ffi::c_void,
            std::mem::size_of::<StardustCreateArg>() as u32,
            std::ptr::null_mut(),
            0,
            &mut returned,
            std::ptr::null_mut(),
        )
    };
    if ok == 0 {
        return Err(Error::MapFailed(format!(
            "CREATE ioctl: {}",
            std::io::Error::last_os_error()
        )));
    }

    let mut mapped = StardustMapOut {
        user_ptr: 0,
        size: 0,
    };
    let ok = unsafe {
        DeviceIoControl(
            h,
            IOCTL_MAP,
            std::ptr::null(),
            0,
            &mut mapped as *mut StardustMapOut as *mut core::ffi::c_void,
            std::mem::size_of::<StardustMapOut>() as u32,
            &mut returned,
            std::ptr::null_mut(),
        )
    };
    if ok == 0 {
        return Err(Error::MapFailed(format!(
            "MAP ioctl: {}",
            std::io::Error::last_os_error()
        )));
    }

    let ptr = mapped.user_ptr as *mut u8;
    let nn =
        NonNull::new(ptr).ok_or_else(|| Error::MapFailed("MAP ioctl returned null VA".into()))?;
    if (nn.as_ptr() as usize) % align != 0 {
        return Err(Error::BadAlign);
    }

    Ok(Region {
        name,
        size,
        align,
        backing: Backing::Module {
            ptr: nn,
            _file: file,
        },
    })
}

#[cfg(windows)]
fn attach_module_os(name: String) -> Result<Region> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle, RawHandle};

    use windows_sys::Win32::Foundation::{HANDLE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem::CreateFileW;
    use windows_sys::Win32::System::IO::DeviceIoControl;

    const FILE_DEVICE_UNKNOWN: u32 = 0x0000_0022;
    const METHOD_BUFFERED: u32 = 0;
    const FILE_ANY_ACCESS: u32 = 0;
    const IOCTL_ATTACH: u32 =
        (FILE_DEVICE_UNKNOWN << 16) | (FILE_ANY_ACCESS << 14) | (0x802 << 2) | METHOD_BUFFERED;
    const IOCTL_MAP: u32 =
        (FILE_DEVICE_UNKNOWN << 16) | (FILE_ANY_ACCESS << 14) | (0x801 << 2) | METHOD_BUFFERED;
    const GENERIC_READ: u32 = 0x8000_0000;
    const GENERIC_WRITE: u32 = 0x4000_0000;
    const FILE_SHARE_READ: u32 = 0x0000_0001;
    const FILE_SHARE_WRITE: u32 = 0x0000_0002;
    const OPEN_EXISTING: u32 = 3;
    const FILE_ATTRIBUTE_NORMAL: u32 = 0x0000_0080;

    #[repr(C)]
    struct StardustCreateArg {
        name: [u8; MODULE_NAME_LEN],
        size: u64,
        align: u64,
    }

    #[repr(C)]
    struct StardustMapOut {
        user_ptr: u64,
        size: u64,
    }

    let encoded = encode_module_name(&name)?;

    let path: Vec<u16> = OsStr::new(r"\\.\stardust")
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let handle = unsafe {
        CreateFileW(
            path.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            std::ptr::null_mut::<core::ffi::c_void>(),
        )
    };
    if handle == INVALID_HANDLE_VALUE || handle.is_null() {
        return Err(Error::MapFailed(format!(
            r"CreateFile \\.\stardust: {}",
            std::io::Error::last_os_error()
        )));
    }
    let file = File::from(unsafe { OwnedHandle::from_raw_handle(handle as RawHandle) });
    let h = file.as_raw_handle() as HANDLE;

    let mut arg = StardustCreateArg {
        name: encoded,
        size: 0,
        align: 0,
    };

    let mut returned: u32 = 0;
    let arg_ptr = &mut arg as *mut StardustCreateArg;
    let ok = unsafe {
        DeviceIoControl(
            h,
            IOCTL_ATTACH,
            arg_ptr as *const core::ffi::c_void,
            std::mem::size_of::<StardustCreateArg>() as u32,
            arg_ptr as *mut core::ffi::c_void,
            std::mem::size_of::<StardustCreateArg>() as u32,
            &mut returned,
            std::ptr::null_mut(),
        )
    };
    if ok == 0 {
        return Err(Error::MapFailed(format!(
            "ATTACH ioctl: {}",
            std::io::Error::last_os_error()
        )));
    }

    let (size, align) = attach_size_align(arg.size, arg.align)?;

    let mut mapped = StardustMapOut {
        user_ptr: 0,
        size: 0,
    };
    let ok = unsafe {
        DeviceIoControl(
            h,
            IOCTL_MAP,
            std::ptr::null(),
            0,
            &mut mapped as *mut StardustMapOut as *mut core::ffi::c_void,
            std::mem::size_of::<StardustMapOut>() as u32,
            &mut returned,
            std::ptr::null_mut(),
        )
    };
    if ok == 0 {
        return Err(Error::MapFailed(format!(
            "MAP ioctl: {}",
            std::io::Error::last_os_error()
        )));
    }

    let ptr = mapped.user_ptr as *mut u8;
    let nn =
        NonNull::new(ptr).ok_or_else(|| Error::MapFailed("MAP ioctl returned null VA".into()))?;
    if (nn.as_ptr() as usize) % align != 0 {
        return Err(Error::BadAlign);
    }

    Ok(Region {
        name,
        size,
        align,
        backing: Backing::Module {
            ptr: nn,
            _file: file,
        },
    })
}

#[cfg(not(any(target_os = "linux", windows)))]
fn map_module_os(_name: String, _size: usize, _align: usize) -> Result<Region> {
    Err(Error::MapFailed(
        r"Region::map_module requires /dev/stardust or \\.\stardust".into(),
    ))
}

#[cfg(not(any(target_os = "linux", windows)))]
fn attach_module_os(_name: String) -> Result<Region> {
    Err(Error::MapFailed(
        r"Region::attach_module requires /dev/stardust or \\.\stardust".into(),
    ))
}

impl Drop for Region {
    fn drop(&mut self) {
        match &mut self.backing {
            Backing::Heap { ptr, layout } => unsafe {
                dealloc(ptr.as_ptr(), *layout);
            },
            #[cfg(any(target_os = "linux", windows))]
            Backing::Module { .. } => {}
            Backing::Anon(_) => {}
            Backing::File { map, .. } => {
                let _ = map.flush();
            }
        }
    }
}

impl std::fmt::Debug for Region {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Region")
            .field("name", &self.name)
            .field("size", &self.size)
            .field("align", &self.align)
            .field("base", &self.as_ptr())
            .finish()
    }
}
