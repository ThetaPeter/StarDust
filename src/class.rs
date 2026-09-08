use std::collections::{BTreeMap, BTreeSet};
#[cfg(feature = "python")]
use std::path::PathBuf;

use crate::arena::Bump;
use crate::error::{Error, Result};
use crate::ir::{Import, Ir, Lang, Param, Ret, Type};
use crate::loader::{CallArg, CallRet, ImportFn, Library};
#[cfg(feature = "python")]
use crate::python::call_python;
use crate::region::Region;

/// Per-method host loader. Not stored in dumpable IR.
pub enum MethodLoader<'a> {
    Library(&'a Library),
    #[cfg(feature = "python")]
    Python {
        script: PathBuf,
    },
}

enum BoundMethod<'a> {
    Library(ImportFn<'a>),
    #[cfg(feature = "python")]
    Python {
        script: PathBuf,
        symbol: String,
    },
}

/// Class methods bound to I1 `Library` and/or I3 Python scripts.
///
/// [`BoundClass::call`] fills `CallArg::Ptr { region: 0, offset }` and
/// `CallArg::Len(layout.inline_size())` (E2a, one region). [`BoundClass::call_with`]
/// appends extra same-region `Scalar` / `Ptr` args (O6). The host does not
/// copy the instance span between methods.
pub struct BoundClass<'a> {
    layout: Type,
    inline_size: u64,
    methods: BTreeMap<String, BoundMethod<'a>>,
}

impl<'a> BoundClass<'a> {
    pub fn bind(
        ir: &Ir,
        class_name: &str,
        loaders: &BTreeMap<String, MethodLoader<'a>>,
    ) -> Result<Self> {
        let class = ir.class(class_name)?;
        let layout = ir.layout(&class.layout)?;
        let inline_size = layout.inline_size();

        let mut methods = BTreeMap::new();
        for (name, import) in merged_methods(ir, class_name)? {
            let loader = loaders
                .get(&name)
                .ok_or_else(|| Error::MissingLoader(name.clone()))?;
            methods.insert(name, bind_method(&import, loader)?);
        }
        Ok(Self {
            layout: layout.clone(),
            inline_size,
            methods,
        })
    }

    /// Bump-allocate an instance of this class's layout in `bump`. Does not write bytes.
    /// Ordinary placement remains caller-chosen `(region, offset)` for [`Self::call`].
    pub fn place(&self, bump: &mut Bump<'_>) -> Result<u64> {
        bump.alloc(&self.layout)
    }

    pub fn call(&self, method: &str, region: &mut Region, offset: u64) -> Result<CallRet> {
        self.call_with(method, region, offset, &[])
    }

    /// Like [`Self::call`], then `extra` after the instance `(Ptr, Len)`.
    ///
    /// `extra` may be [`CallArg::Scalar`] and [`CallArg::Ptr`] with `region: 0`
    /// only. [`CallArg::Len`] or a `Ptr` into another region is an error.
    pub fn call_with(
        &self,
        method: &str,
        region: &mut Region,
        offset: u64,
        extra: &[CallArg],
    ) -> Result<CallRet> {
        check_class_extra(extra)?;
        match self.methods.get(method) {
            None => Err(Error::UnknownMethod),
            Some(BoundMethod::Library(func)) => {
                let mut args = Vec::with_capacity(2 + extra.len());
                args.push(CallArg::Ptr { region: 0, offset });
                args.push(CallArg::Len(self.inline_size));
                args.extend_from_slice(extra);
                func.call(region, &args)
            }
            #[cfg(feature = "python")]
            Some(BoundMethod::Python { script, symbol }) => {
                if !extra.is_empty() {
                    return Err(Error::UnsupportedCall);
                }
                call_python(region, offset, self.inline_size, script, symbol)?;
                Ok(CallRet::Void)
            }
        }
    }
}

fn check_class_extra(extra: &[CallArg]) -> Result<()> {
    for arg in extra {
        match arg {
            CallArg::Scalar(_) => {}
            CallArg::Ptr { region: 0, .. } => {}
            CallArg::Ptr { .. } => return Err(Error::TypeMismatch),
            CallArg::Len(_) => return Err(Error::UnsupportedCall),
        }
    }
    Ok(())
}

/// Walk the base chain root-first; later classes overwrite method names.
fn merged_methods(ir: &Ir, class_name: &str) -> Result<BTreeMap<String, Import>> {
    let mut chain = Vec::new();
    let mut seen = BTreeSet::new();
    let mut name = class_name;
    loop {
        if !seen.insert(name.to_string()) {
            return Err(Error::InvalidLayout);
        }
        let class = ir.class(name)?;
        chain.push(class);
        match class.base.as_deref() {
            None => break,
            Some(base) => name = base,
        }
    }
    let mut methods = BTreeMap::new();
    for class in chain.into_iter().rev() {
        for (method, import) in &class.methods {
            methods.insert(method.clone(), import.clone());
        }
    }
    Ok(methods)
}

fn bind_method<'a>(import: &Import, loader: &MethodLoader<'a>) -> Result<BoundMethod<'a>> {
    match import.lang {
        Lang::Python => bind_python(import, loader),
        Lang::C | Lang::Llvm | Lang::Haskell | Lang::Lean => bind_library(import, loader),
    }
}

fn bind_library<'a>(import: &Import, loader: &MethodLoader<'a>) -> Result<BoundMethod<'a>> {
    match loader {
        MethodLoader::Library(lib) => {
            check_library_sig(import)?;
            Ok(BoundMethod::Library(lib.bind_import(import)?))
        }
        #[cfg(feature = "python")]
        MethodLoader::Python { .. } => Err(Error::TypeMismatch),
    }
}

#[cfg(feature = "python")]
fn bind_python<'a>(import: &Import, loader: &MethodLoader<'a>) -> Result<BoundMethod<'a>> {
    match loader {
        MethodLoader::Python { script } => {
            check_python_sig(import)?;
            Ok(BoundMethod::Python {
                script: script.clone(),
                symbol: import.symbol.clone(),
            })
        }
        MethodLoader::Library(_) => Err(Error::TypeMismatch),
    }
}

#[cfg(not(feature = "python"))]
fn bind_python<'a>(_import: &Import, _loader: &MethodLoader<'a>) -> Result<BoundMethod<'a>> {
    Err(Error::Unsupported { family: "python" })
}

fn check_library_sig(import: &Import) -> Result<()> {
    if matches!(import.ret, Ret::OwnedPtr)
        || import.args.iter().any(|p| matches!(p, Param::OwnedPtr))
    {
        return Err(Error::IncompatibleImport);
    }
    match import.args.as_slice() {
        [Param::Ptr, Param::Len] => Ok(()),
        [Param::Ptr, Param::Len, rest @ ..] => {
            if rest
                .iter()
                .all(|p| matches!(p, Param::Scalar(_) | Param::Ptr))
            {
                Ok(())
            } else {
                Err(Error::UnsupportedCall)
            }
        }
        _ => Err(Error::IncompatibleImport),
    }
}

#[cfg(feature = "python")]
fn check_python_sig(import: &Import) -> Result<()> {
    if !matches!(import.ret, Ret::Void) || import.args.as_slice() != [Param::Ptr, Param::Len] {
        return Err(Error::UnsupportedCall);
    }
    Ok(())
}
