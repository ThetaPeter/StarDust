# StarDust

A **Rust-hosted library** for laying out named byte regions and calling existing foreign functions on those bytes. It is **not** a general-purpose language, and not a rewrite of C, Python, LLVM, Haskell, or Rust into StarDust syntax.

Dumpable IR is the source of truth; `.sd` is a copy.

Two thin layers:

1. **To the OS** — how bytes are stored, retrieved, and represented (typed views on the same named region, no copy).
2. **To others** — call C, Python, LLVM, Haskell, and Rust exports on those bytes without translating the callee into Rust types and without pre/post-processing when the payload is a StarDust region.

## Two layers

1. **Layout** — named regions of bytes, with three verbs:
    - **Stored** — the bytes sit in a region.
    - **Retrieved** — named field paths / offsets into that region.
    - **Represented** — a typed view over the **same** bytes (no copy). If two views disagree, that is an error, not a conversion.
2. **Imports** — call functions that already exist. Do not translate the callee into Rust types. Do not pre- or post-process when the payload is a StarDust region.

### Region-compatible calls

A call is region-compatible when every argument and return is:

- a scalar (integer or float by value), or
- a pointer into a live StarDust region (`base + offset`), or
- a length/size for those spans, or
- a POD struct whose fields are the above.

Incompatible signatures **fail** import. There is no silent copy.

## Languages

Imports are C, Python, LLVM, Haskell, and Rust. Lean is **not** an import.

- **Python** is in-process CPython plus a writable `memoryview` over the region.
- **C, LLVM, Haskell, and Rust** load as a platform `.dll` / `.so` export. Call existing functions; the callee does not compile against StarDust headers.

## Class, inheritance, export

- A **class** is a layout name plus a method table. Method bodies stay foreign. One class may mix languages.
- **Class inheritance** is single: the derived layout is a **prefix** of the base. Override rebinds the same method name to another import.
- **`>>` exports** live on the **exporter** class (fields with assign/retrieve, and methods). Any instance that `link`s a named `link` slot to that class sees those names on the host `Handle`. Assign/retrieve and export methods receive the database span only. Owned bytes stay the layout. This is not a second inheritance.

Private fields, static methods, and StarDust-written method bodies are not locked. Abstract methods and `super` are later.

## Syntax

Three declaration surfaces describe the same IR. Dumpable IR is authority. `.sd` is a thin syntax copy. The Rust builder produces that IR directly.

This chapter covers those three surfaces only.

- **`.sd`** — write layouts, imports, and classes as a file (Inspect, tests, examples). Load with `Ir::from_sd` / `Ir::from_sd_path`.
- **Dumpable IR (JSON)** — the host’s source of truth, including field `offset` / `size` / `align`. Dump and reload with `to_json` / `from_json`. Use this when you need placement numbers, or when `to_sd` cannot print sugar.
- **Rust builder** — the same declarations from a host program or test, without a `.sd` file.

### `.sd`

The file starts with `endian little` or `endian big` (how multi-byte scalars are stored). Optional `bit_order lsb0` follows (`msb0` is unsupported): bitpack fields are numbered from the least-significant bit. Then any mix of `layout`, `import`, and `class`. Nested structs are inline `struct repr_c { ... }` (no named type refs in field position).

**Layouts** name a byte arrangement you can store, retrieve, and view on a region.

- `layout Name repr_c { ... }` — C-like POD: each field is aligned, the struct size is rounded up. Use this for ordinary records you pass as `base + offset`.
- `layout Name columnar(N) { ... }` — a table of `N` row slots (SoA). Count sits at the front; each column is a contiguous array. Use this when you want columnar access instead of row structs.
- `layout Name bitpack u8|u16|u32|u64 { field: width; }` — packed bits inside one unsigned cell. Use this for flag words and other dense integer overlays.
- `layout Name bitpack span N { field: width; }` — packed bits over `N` bytes with no integer container. Use this for a free bitstream. Do not mix a container with `span`.

```
endian little
bit_order lsb0

layout Packet repr_c {
  n: u32;
  inner: struct repr_c {
    a: u8;
    b: i16;
  };
  arr: [u16; 3];
  xs: slice u32;
  extras: link;
}

layout Table columnar(8) {
  id: u32;
  flag: u8;
}

layout Flags bitpack u32 {
  a: 1;
  b: 3;
}

layout Frame bitpack span 3 {
  a: 12;
  b: 12;
}
```

Field types (what sits in the bytes):

- scalars `i8` `u8` `i16` `u16` `i32` `u32` `i64` `u64` `f32` `f64` — fixed-width integers and floats.
- `struct repr_c { ... }` — a nested POD record (inline; it does not name a second layout).
- `[T; n]` — a fixed inline array.
- `slice T` — an offset plus length into the **same** region (variable-length payload).
- `link` — a `u64` byte offset from the region base (`0` is null). Named `link` slots store `Handle::link` targets; `0` means unlinked. Not a process pointer.

Column cells are a scalar or an array of scalars.

**Imports** name an existing foreign function. StarDust binds the symbol and calls it on region bytes; it does not translate the callee into Rust types.

```
endian little

import write_a(ptr, len) -> void {
  symbol write_a;
}

import plus(ptr, len, u64) -> u64 {
  symbol plus_u64;
  lang rust;
}
```

- `import Name(args) -> ret` — the StarDust name you bind and call.
- `symbol ident` — the export in the `.dll` / `.so` / Python module.
- `lang c|python|llvm|haskell|rust` — which loader opens that symbol (`c` is the default if omitted).
- `ptr` — a pointer into a live region (`base + offset`).
- `len` — the byte or element length of that span.
- a scalar in the arg/ret list — passed by value.
- `void` — no return value.
- `owned_ptr` — a callee-owned pointer. It parses so the file can describe the signature; bind still fails as incompatible (no silent adopt of `malloc`).

**Class, base, export** attach a method table (and optional `>>` exports) to a layout. Method bodies stay foreign. Use-site is host Rust (`Handle`), not `.sd` expressions.

```
endian little

layout BaseLay repr_c {
  x: u32;
}

layout DerivedLay repr_c {
  x: u32;
  y: u32;
}

layout StudentLay repr_c {
  major: u32;
  db: link;
}

layout DbLay repr_c {
  len: u64;
  buf: [u8; 32];
}

layout MinorLay repr_c {
  data: [u8; 8];
}

class Base {
  layout BaseLay;
  method paint(ptr, len) -> void {
    symbol write_a;
    lang c;
  }
}

class Derived {
  layout DerivedLay;
  base Base;
  method paint(ptr, len) -> void {
    symbol mut_b;
    lang c;
  }
}

class StudentMinorDatabase {
  layout DbLay;
  >> minor : MinorLay {
    assign (ptr, len, ptr, len) -> void {
      symbol set_minor;
      lang rust;
    }
    retrieve (ptr, len, ptr, len) -> void {
      symbol get_minor;
      lang rust;
    }
  }
  >> print_minor(ptr, len) -> void {
    symbol print_minor;
    lang rust;
  }
}

class Student {
  layout StudentLay;
}
```

- `class Name { ... }` — a named layout plus methods (and `>>` exports). Use this to call foreign functions as methods on a region offset, or to declare exports that appear on a linked handle.
- `layout LayoutName` — which bytes a bound instance views. Required. Owned fields stay here. Do not add `->` as a field marker.
- `base OtherClass` — single inheritance. The derived layout must be a **prefix** of the base (same leading fields). Override is a `method` with the same name rebound to another import. Dispatch uses the **declared** class of the view.
- `method name(args) -> ret { symbol ...; lang ...; }` — bind that method name to a foreign symbol. One class may mix languages. Non-export methods stay on that class's handle.
- `>> name : LayoutName { assign SIG { ... } retrieve SIG { ... } }` — an export field on the **exporter** class. Any instance that `link`s to it may assign/retrieve that name. Both signatures are `(ptr, len, ptr, len) -> void` (database span, then a value span of `LayoutName` size). Host: `Handle::field_assign` / `field_retrieve`.
- `>> name(ptr, len) -> void { symbol ...; lang ...; }` — an export method. `(ptr, len)` is the database span. Host: `Handle::call_export`. The exporter's own `call` does not see export names.
- `Handle::link("slot", db)` — `slot` is a `link` field on the **caller** layout. `0` is unlinked (`Unlinked` on later export use). Two linked targets that export the same name error at `link`.

Print with `Ir::to_sd`. `from_sd(to_sd(ir))` equals `ir` for first-layer IR.

### Dumpable IR

`Ir` is the serde document: `endianness`, `bit_order`, `layouts`, `imports`, `classes`. Each field also stores placement (`offset`, `size`, `align`) so a dump is enough to retrieve and view without re-running a builder. JSON is `Ir::to_json` / `Ir::to_json_pretty` / `Ir::from_json`.

```json
{
  "endianness": "Little",
  "bit_order": "Lsb0",
  "layouts": {
    "Packet": {
      "Struct": {
        "fields": [
          {"name": "n", "ty": {"Scalar": "U32"}, "offset": 0, "size": 4, "align": 4}
        ],
        "size": 4,
        "align": 4
      }
    }
  },
  "imports": {},
  "classes": {}
}
```

`to_sd()` round-trips the same IR when every layout equals the builder rebuild (`repr_c` / `ColumnarBuilder` / `BitpackBuilder`). Otherwise `to_sd` returns `Error::Dsl` and JSON dump still works.

### Rust builder

The same IR as the `.sd` forms above, for tests and host code:

- `Ir::new` / `insert` / `insert_import` / `insert_class` — assemble the dumpable document.
- `StructBuilder::field` + `repr_c` — POD placement.
- `ColumnarBuilder::column` + `build` — SoA table of a fixed capacity.
- `BitpackBuilder::new` + `field` + `build` — bits in an unsigned cell; `BitpackBuilder::span` for a free bitstream.
- `Import::new` / `with_lang` — a foreign signature.
- `Class` / `insert_method` / `insert_export_field` / `insert_export_method` — method table and `>>` exports.

```rust
use stardust::{
    BitpackBuilder, Class, ColumnarBuilder, Endianness, ExportField, Import, Ir, Lang, Param, Ret,
    Scalar, StructBuilder, Type,
};

let packet = StructBuilder::new()
    .field("n", Type::scalar(Scalar::U32))
    .repr_c()?;

let table = ColumnarBuilder::new(8)
    .column("id", Type::scalar(Scalar::U32))?
    .column("flag", Type::scalar(Scalar::U8))?
    .build()?;

let flags = BitpackBuilder::new(Scalar::U32)
    .field("a", 1)
    .field("b", 3)
    .build()?;

let frame = BitpackBuilder::span(3)?
    .field("a", 12)
    .field("b", 12)
    .build()?;

let mut ir = Ir::new(Endianness::Little);
ir.insert("Packet", packet);
ir.insert("Table", table);
ir.insert("Flags", flags);
ir.insert("Frame", frame);
ir.insert_import(
    "write_a",
    Import::new("write_a", vec![Param::Ptr, Param::Len], Ret::Void),
)?;

let student_lay = StructBuilder::new()
    .field("major", Type::scalar(Scalar::U32))
    .field("db", Type::link())
    .repr_c()?;
let db_lay = StructBuilder::new()
    .field("len", Type::scalar(Scalar::U64))
    .field("buf", Type::array(Type::scalar(Scalar::U8), 32)?)
    .repr_c()?;
let minor_lay = StructBuilder::new()
    .field("data", Type::array(Type::scalar(Scalar::U8), 8)?)
    .repr_c()?;
ir.insert("StudentLay", student_lay);
ir.insert("DbLay", db_lay);
ir.insert("MinorLay", minor_lay);
let mut db = Class {
    layout: "DbLay".into(),
    base: None,
    methods: Default::default(),
    export_fields: Default::default(),
    export_methods: Default::default(),
};
db.insert_export_field(
    "minor",
    ExportField {
        layout: "MinorLay".into(),
        assign: Import::new(
            "set_minor",
            vec![Param::Ptr, Param::Len, Param::Ptr, Param::Len],
            Ret::Void,
        )
        .with_lang(Lang::Rust),
        retrieve: Import::new(
            "get_minor",
            vec![Param::Ptr, Param::Len, Param::Ptr, Param::Len],
            Ret::Void,
        )
        .with_lang(Lang::Rust),
    },
)?;
db.insert_export_method(
    "print_minor",
    Import::new("print_minor", vec![Param::Ptr, Param::Len], Ret::Void).with_lang(Lang::Rust),
)?;
ir.insert_class("StudentMinorDatabase", db)?;
ir.insert_class(
    "Student",
    Class {
        layout: "StudentLay".into(),
        base: None,
        methods: Default::default(),
        export_fields: Default::default(),
        export_methods: Default::default(),
    },
)?;
```

## Build and test

```
cargo test
```

Default `cargo test` is gcc/clang userspace only. Optional features: `python`, `llvm`, `haskell`, `kernel` (Linux `/dev/stardust` or Windows `\\.\stardust` when present; otherwise tests skip).

Kernel backing is optional. Linux: `/dev/stardust`. Windows recipe: [kmod/stardust-kmdf/README.md](kmod/stardust-kmdf/README.md) (`\\.\stardust`).

## Where to read

- [outline.md](outline.md) — product intent (not executable)
- [waves.md](waves.md) — live contract (locks, parked gaps, named tests) and the archive of done L/I/R/O I/O/B
- [kmod/stardust-kmdf/README.md](kmod/stardust-kmdf/README.md) — Windows driver install
- [examples/cns_graph](examples/cns_graph) — CNS graph + Luna Light (Python BoundClass; `cargo test --features python --test cns`)
- [examples/packet](examples/packet) — Frame / EthFrame prefix, C + Python + Rust methods, C `>>` seal (`cargo test --features python --test packet`)
