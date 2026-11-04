use std::ffi::CString;
use std::path::Path;

use pyo3::ffi;
use pyo3::prelude::*;
use pyo3::types::PyMemoryView;

use crate::error::{Error, Result};
use crate::region::Region;

/// Invoke `callable` from a Python source file with a writable memoryview over
/// `[offset, offset+len)` of `region`. The view wraps `base + offset` in place
/// (`PyBUF_WRITE`); the host does not copy the span into `bytes` / `bytearray`.
///
/// The memoryview is released before this function returns. `region` must stay
/// alive for the duration of the call.
pub fn call_python(
    region: &mut Region,
    offset: u64,
    len: u64,
    script: impl AsRef<Path>,
    callable: &str,
) -> Result<()> {
    let offset = usize::try_from(offset).map_err(|_| Error::OutOfBounds)?;
    let len = usize::try_from(len).map_err(|_| Error::OutOfBounds)?;
    let end = offset.checked_add(len).ok_or(Error::Overflow)?;
    if end > region.size() {
        return Err(Error::OutOfBounds);
    }

    let path = script.as_ref();
    let source = std::fs::read_to_string(path)
        .map_err(|e| Error::LoadFailed(format!("{}: {e}", path.display())))?;
    let code =
        CString::new(source).map_err(|e| Error::LoadFailed(format!("{}: {e}", path.display())))?;
    let file_name = CString::new(path.to_string_lossy().as_bytes())
        .map_err(|e| Error::LoadFailed(format!("{}: {e}", path.display())))?;
    let module_name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("stardust_py");
    let module_name = CString::new(module_name)
        .map_err(|e| Error::LoadFailed(format!("{}: {e}", path.display())))?;

    let ptr = unsafe { region.as_mut_ptr().add(offset) };
    Python::attach(|py| {
        let module = PyModule::from_code(py, &code, &file_name, &module_name)
            .map_err(|e| Error::Python(e.to_string()))?;
        let func = module
            .getattr(callable)
            .map_err(|e| Error::Python(e.to_string()))?;

        let view_ptr =
            unsafe { ffi::PyMemoryView_FromMemory(ptr.cast(), len as isize, ffi::PyBUF_WRITE) };
        let view = unsafe { Bound::<PyAny>::from_owned_ptr_or_err(py, view_ptr) }
            .map_err(|e| Error::Python(e.to_string()))?
            .cast_into::<PyMemoryView>()
            .map_err(|e| Error::Python(e.to_string()))?;

        let result = func
            .call1((&view,))
            .map_err(|e| Error::Python(e.to_string()));
        let _ = view.call_method0("release");
        result.map(|_| ())
    })
}
