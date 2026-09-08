use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    UnknownPath,
    OutOfBounds,
    BadAlign,
    UnsupportedEndian,
    UnsupportedBitOrder,
    UnknownLayout,
    TypeMismatch,
    Alloc,
    Json(String),
    Dsl(String),
    Overflow,
    DuplicateField,
    BadColumnType,
    InvalidLayout,
    LoadFailed(String),
    MissingSymbol(String),
    IncompatibleImport,
    UnsupportedCall,
    DuplicateImport,
    DuplicateClass,
    DuplicateMethod,
    UnknownImport,
    UnknownClass,
    UnknownMethod,
    MissingLoader(String),
    MapFailed(String),
    Unsupported { family: &'static str },
    PinFailed(String),
    PageControl(String),
    Python(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::UnknownPath => write!(f, "unknown field path"),
            Error::OutOfBounds => write!(f, "offset or index is out of bounds"),
            Error::BadAlign => write!(f, "invalid or unsatisfied alignment"),
            Error::UnsupportedEndian => write!(f, "big-endian layouts are not supported in W1"),
            Error::UnsupportedBitOrder => write!(f, "msb0 bit order is not supported"),
            Error::UnknownLayout => write!(f, "unknown layout name"),
            Error::TypeMismatch => write!(f, "value does not match layout type"),
            Error::Alloc => write!(f, "region allocation failed"),
            Error::Json(msg) => write!(f, "IR JSON error: {msg}"),
            Error::Dsl(msg) => write!(f, "DSL error: {msg}"),
            Error::Overflow => write!(f, "offset or size overflow"),
            Error::DuplicateField => write!(f, "duplicate field name"),
            Error::BadColumnType => write!(
                f,
                "column cell type must be a scalar or a one-level array of scalars"
            ),
            Error::InvalidLayout => write!(f, "invalid layout"),
            Error::LoadFailed(msg) => write!(f, "failed to load library: {msg}"),
            Error::MissingSymbol(sym) => write!(f, "missing imported symbol: {sym}"),
            Error::IncompatibleImport => write!(f, "import is not region-compatible"),
            Error::UnsupportedCall => write!(f, "call shape is not supported"),
            Error::DuplicateImport => write!(f, "duplicate import name"),
            Error::DuplicateClass => write!(f, "duplicate class name"),
            Error::DuplicateMethod => write!(f, "duplicate method name"),
            Error::UnknownImport => write!(f, "unknown import name"),
            Error::UnknownClass => write!(f, "unknown class name"),
            Error::UnknownMethod => write!(f, "unknown method name"),
            Error::MissingLoader(name) => write!(f, "missing loader for method: {name}"),
            Error::MapFailed(msg) => write!(f, "memory map failed: {msg}"),
            Error::Unsupported { family } => {
                write!(f, "unsupported page control: {family}")
            }
            Error::PinFailed(msg) => write!(f, "pin failed: {msg}"),
            Error::PageControl(msg) => write!(f, "page control failed: {msg}"),
            Error::Python(msg) => write!(f, "python error: {msg}"),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;
