use std::ffi::CString;
use std::marker::PhantomData;

use libffi::low::CodePtr;
use libffi::middle::{arg, Cif, Type};

use crate::error::{Error, Result};
use crate::ir::{Import, Ir, Param, Ret, Scalar};
use crate::region::Region;

/// Host argument to [`ImportFn::call`] / [`ImportFn::call_regions`].
///
/// `Ptr.region` is an index into the slice passed to [`ImportFn::call_regions`].
/// [`ImportFn::call`] requires every `Ptr.region` to be `0`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CallArg {
    Ptr { region: usize, offset: u64 },
    Len(u64),
    Scalar(CallScalar),
}

/// Scalar payload for [`CallArg`] / [`CallRet`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CallScalar {
    I8(i8),
    U8(u8),
    I16(i16),
    U16(u16),
    I32(i32),
    U32(u32),
    I64(i64),
    U64(u64),
    F32(f32),
    F64(f64),
}

impl CallScalar {
    fn ty(self) -> Scalar {
        match self {
            CallScalar::I8(_) => Scalar::I8,
            CallScalar::U8(_) => Scalar::U8,
            CallScalar::I16(_) => Scalar::I16,
            CallScalar::U16(_) => Scalar::U16,
            CallScalar::I32(_) => Scalar::I32,
            CallScalar::U32(_) => Scalar::U32,
            CallScalar::I64(_) => Scalar::I64,
            CallScalar::U64(_) => Scalar::U64,
            CallScalar::F32(_) => Scalar::F32,
            CallScalar::F64(_) => Scalar::F64,
        }
    }
}

/// Host return from [`ImportFn::call`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CallRet {
    Void,
    Scalar(CallScalar),
}

/// Loaded C shared library.
///
/// Bind each import, then [`ImportFn::call`] them sequentially on the same
/// [`Region`]. Sequential calls compose imports in place: the host does not
/// copy or re-encode the payload between calls.
pub struct Library {
    inner: libloading::Library,
}

pub struct ImportFn<'lib> {
    _lib: PhantomData<&'lib Library>,
    code: CodePtr,
    cif: Cif,
    import: Import,
}

impl std::fmt::Debug for ImportFn<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImportFn").finish_non_exhaustive()
    }
}

enum FfiSlot {
    Pointer(*mut u8),
    Usize(usize),
    I8(i8),
    U8(u8),
    I16(i16),
    U16(u16),
    I32(i32),
    U32(u32),
    I64(i64),
    U64(u64),
    F32(f32),
    F64(f64),
}

impl Library {
    pub fn load(path: impl AsRef<std::path::Path>) -> Result<Self> {
        let path = path.as_ref();
        let inner = unsafe { libloading::Library::new(path) }
            .map_err(|e| Error::LoadFailed(format!("{}: {e}", path.display())))?;
        Ok(Self { inner })
    }

    pub fn bind(&self, ir: &Ir, name: &str) -> Result<ImportFn<'_>> {
        self.bind_import(ir.import(name)?)
    }

    /// Bind a class method (or any) `Import` without looking up `Ir.imports`.
    pub fn bind_import(&self, import: &Import) -> Result<ImportFn<'_>> {
        if has_owned_ptr(import) {
            return Err(Error::IncompatibleImport);
        }
        let cname = CString::new(import.symbol.as_str())
            .map_err(|_| Error::MissingSymbol(import.symbol.clone()))?;
        let symbol: libloading::Symbol<unsafe extern "C" fn()> =
            unsafe { self.inner.get(cname.as_bytes_with_nul()) }
                .map_err(|_| Error::MissingSymbol(import.symbol.clone()))?;
        let code = CodePtr(unsafe { symbol.into_raw().as_raw_ptr() });
        let arg_types: Vec<Type> = import.args.iter().map(ffi_type_param).collect();
        let cif = Cif::new(arg_types, ffi_type_ret(&import.ret));
        Ok(ImportFn {
            _lib: PhantomData,
            code,
            cif,
            import: import.clone(),
        })
    }
}

impl ImportFn<'_> {
    /// Call this import in place on `region` (`base + offset`).
    ///
    /// Every [`CallArg::Ptr`] must have `region: 0`; any other index is
    /// [`Error::TypeMismatch`]. Sequential calls on one region compose imports.
    /// The host does not copy or re-encode the payload between calls.
    pub fn call(&self, region: &mut Region, args: &[CallArg]) -> Result<CallRet> {
        for arg in args {
            if let CallArg::Ptr { region: idx, .. } = *arg {
                if idx != 0 {
                    return Err(Error::TypeMismatch);
                }
            }
        }
        self.call_regions(&mut [region], args)
    }

    /// Call this import across `regions` without copying region bytes.
    ///
    /// Each [`CallArg::Ptr`] `region` indexes this slice (`Error::OutOfBounds`
    /// if `>= len`). Two offsets in one region belong on [`Self::call`] (both
    /// `region: 0`), not two slice entries for the same object.
    pub fn call_regions(&self, regions: &mut [&mut Region], args: &[CallArg]) -> Result<CallRet> {
        if args.len() != self.import.args.len() {
            return Err(Error::TypeMismatch);
        }
        check_ptr_len_spans(regions, &self.import.args, args)?;

        let mut slots: Vec<FfiSlot> = Vec::with_capacity(args.len());
        for (param, arg) in self.import.args.iter().zip(args.iter()) {
            slots.push(slot_from_arg(regions, param, arg)?);
        }
        let ffi_args: Vec<_> = slots.iter().map(arg_from_slot).collect();
        unsafe { invoke(&self.cif, self.code, &ffi_args, &self.import.ret) }
    }
}

fn has_owned_ptr(import: &Import) -> bool {
    matches!(import.ret, Ret::OwnedPtr) || import.args.iter().any(|p| matches!(p, Param::OwnedPtr))
}

fn ffi_type_param(p: &Param) -> Type {
    match p {
        Param::Ptr => Type::pointer(),
        Param::Len => ffi_type_usize(),
        Param::Scalar(s) => ffi_type_scalar(*s),
        Param::OwnedPtr => Type::pointer(),
    }
}

fn ffi_type_ret(r: &Ret) -> Type {
    match r {
        Ret::Void => Type::void(),
        Ret::Scalar(s) => ffi_type_scalar(*s),
        Ret::OwnedPtr => Type::pointer(),
    }
}

fn ffi_type_usize() -> Type {
    if usize::BITS == 64 {
        Type::u64()
    } else {
        Type::u32()
    }
}

fn ffi_type_scalar(s: Scalar) -> Type {
    match s {
        Scalar::I8 => Type::i8(),
        Scalar::U8 => Type::u8(),
        Scalar::I16 => Type::i16(),
        Scalar::U16 => Type::u16(),
        Scalar::I32 => Type::i32(),
        Scalar::U32 => Type::u32(),
        Scalar::I64 => Type::i64(),
        Scalar::U64 => Type::u64(),
        Scalar::F32 => Type::f32(),
        Scalar::F64 => Type::f64(),
    }
}

fn check_ptr_len_spans(regions: &[&mut Region], params: &[Param], args: &[CallArg]) -> Result<()> {
    for i in 0..params.len() {
        if !matches!(params[i], Param::Ptr) {
            continue;
        }
        let (idx, offset) = match args[i] {
            CallArg::Ptr { region, offset } => {
                let offset = usize::try_from(offset).map_err(|_| Error::OutOfBounds)?;
                (region, offset)
            }
            _ => return Err(Error::TypeMismatch),
        };
        let region = regions.get(idx).ok_or(Error::OutOfBounds)?;
        if offset > region.size() {
            return Err(Error::OutOfBounds);
        }
        if params.get(i + 1).is_some_and(|p| matches!(p, Param::Len)) {
            let len = match args[i + 1] {
                CallArg::Len(len) => usize::try_from(len).map_err(|_| Error::OutOfBounds)?,
                _ => return Err(Error::TypeMismatch),
            };
            let end = offset.checked_add(len).ok_or(Error::Overflow)?;
            if end > region.size() {
                return Err(Error::OutOfBounds);
            }
        }
    }
    Ok(())
}

fn slot_from_arg(regions: &mut [&mut Region], param: &Param, arg: &CallArg) -> Result<FfiSlot> {
    match (param, arg) {
        (
            Param::Ptr,
            CallArg::Ptr {
                region: idx,
                offset,
            },
        ) => {
            let offset = usize::try_from(*offset).map_err(|_| Error::OutOfBounds)?;
            let region = regions.get_mut(*idx).ok_or(Error::OutOfBounds)?;
            Ok(FfiSlot::Pointer(unsafe { region.as_mut_ptr().add(offset) }))
        }
        (Param::Len, CallArg::Len(len)) => {
            let len = usize::try_from(*len).map_err(|_| Error::OutOfBounds)?;
            Ok(FfiSlot::Usize(len))
        }
        (Param::Scalar(want), CallArg::Scalar(got)) if got.ty() == *want => {
            Ok(slot_from_scalar(*got))
        }
        _ => Err(Error::TypeMismatch),
    }
}

fn slot_from_scalar(s: CallScalar) -> FfiSlot {
    match s {
        CallScalar::I8(v) => FfiSlot::I8(v),
        CallScalar::U8(v) => FfiSlot::U8(v),
        CallScalar::I16(v) => FfiSlot::I16(v),
        CallScalar::U16(v) => FfiSlot::U16(v),
        CallScalar::I32(v) => FfiSlot::I32(v),
        CallScalar::U32(v) => FfiSlot::U32(v),
        CallScalar::I64(v) => FfiSlot::I64(v),
        CallScalar::U64(v) => FfiSlot::U64(v),
        CallScalar::F32(v) => FfiSlot::F32(v),
        CallScalar::F64(v) => FfiSlot::F64(v),
    }
}

fn arg_from_slot(slot: &FfiSlot) -> libffi::middle::Arg {
    match slot {
        FfiSlot::Pointer(p) => arg(p),
        FfiSlot::Usize(v) => arg(v),
        FfiSlot::I8(v) => arg(v),
        FfiSlot::U8(v) => arg(v),
        FfiSlot::I16(v) => arg(v),
        FfiSlot::U16(v) => arg(v),
        FfiSlot::I32(v) => arg(v),
        FfiSlot::U32(v) => arg(v),
        FfiSlot::I64(v) => arg(v),
        FfiSlot::U64(v) => arg(v),
        FfiSlot::F32(v) => arg(v),
        FfiSlot::F64(v) => arg(v),
    }
}

unsafe fn invoke(
    cif: &Cif,
    code: CodePtr,
    ffi_args: &[libffi::middle::Arg],
    ret: &Ret,
) -> Result<CallRet> {
    Ok(match ret {
        Ret::Void => {
            cif.call::<()>(code, ffi_args);
            CallRet::Void
        }
        Ret::OwnedPtr => return Err(Error::IncompatibleImport),
        Ret::Scalar(Scalar::I8) => CallRet::Scalar(CallScalar::I8(cif.call(code, ffi_args))),
        Ret::Scalar(Scalar::U8) => CallRet::Scalar(CallScalar::U8(cif.call(code, ffi_args))),
        Ret::Scalar(Scalar::I16) => CallRet::Scalar(CallScalar::I16(cif.call(code, ffi_args))),
        Ret::Scalar(Scalar::U16) => CallRet::Scalar(CallScalar::U16(cif.call(code, ffi_args))),
        Ret::Scalar(Scalar::I32) => CallRet::Scalar(CallScalar::I32(cif.call(code, ffi_args))),
        Ret::Scalar(Scalar::U32) => CallRet::Scalar(CallScalar::U32(cif.call(code, ffi_args))),
        Ret::Scalar(Scalar::I64) => CallRet::Scalar(CallScalar::I64(cif.call(code, ffi_args))),
        Ret::Scalar(Scalar::U64) => CallRet::Scalar(CallScalar::U64(cif.call(code, ffi_args))),
        Ret::Scalar(Scalar::F32) => CallRet::Scalar(CallScalar::F32(cif.call(code, ffi_args))),
        Ret::Scalar(Scalar::F64) => CallRet::Scalar(CallScalar::F64(cif.call(code, ffi_args))),
    })
}
