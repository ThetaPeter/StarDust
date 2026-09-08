//! StarDust: dumpable layout IR, named regions, typed views, and region-compatible C imports.

mod access;
mod arena;
mod builder;
mod class;
mod dsl;
mod error;
mod ir;
mod loader;
mod page;
mod path;
#[cfg(feature = "python")]
mod python;
mod region;
mod slot;
mod value;
mod view;

pub use arena::Bump;
pub use slot::SlotAlloc;
pub use builder::{BitpackBuilder, ColumnarBuilder, StructBuilder};
pub use class::{BoundClass, MethodLoader};
pub use error::{Error, Result};
pub use ir::{
    ArrayType, BitField, BitOrder, BitpackType, Class, ColumnarType, Endianness, Field, Import, Ir,
    Lang, Param, Ret, Scalar, SliceType, StructType, Type,
};
pub use loader::{CallArg, CallRet, CallScalar, ImportFn, Library};
pub use page::Advice;
pub use path::{Path, Seg};
#[cfg(feature = "python")]
pub use python::call_python;
pub use region::Region;
pub use value::Value;
pub use view::{ColumnCell, ColumnView, View};
