#[derive(Debug, Clone, PartialEq)]
pub enum Value {
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
    Struct(Vec<(String, Value)>),
    Array(Vec<Value>),
    Slice { offset: u64, elems: Vec<Value> },
}

macro_rules! from_scalar {
    ($($ty:ty => $var:ident),+ $(,)?) => {
        $(
            impl From<$ty> for Value {
                fn from(v: $ty) -> Self {
                    Value::$var(v)
                }
            }
        )+
    };
}

from_scalar!(
    i8 => I8,
    u8 => U8,
    i16 => I16,
    u16 => U16,
    i32 => I32,
    u32 => U32,
    i64 => I64,
    u64 => U64,
    f32 => F32,
    f64 => F64,
);
