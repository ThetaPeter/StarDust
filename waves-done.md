# StarDust done waves (archive)

This file is **not** the executable book. Do not start these waves; they are `done`. Locks, parked gaps, and named tests live in [waves.md](waves.md).

Archive: Layout **L1–L6**, Import **I1–I7**, Region **R1–R6**, OOP **O1–O6** (former W1–W16 plus later host waves).

**G1–G7 aliases (this archive only):** G1=I5, G2=I6, G3=L3, G4=L4, G5=L5, G6=R4, G7=R5. These are not [choices.md](choices.md) G1–G5 (OOP extras).

**Tests:** named tests and skip-or-fail rules: [waves.md](waves.md) Named tests.

---

## Done — Layout

Former waves grouped here. Status `done`. Do not re-execute.

## Wave L1 — IR + heap region

**Was:** W1. Tests: [`tests/layout.rs`](tests/layout.rs).

**Status:** `done`

### Input

- Empty library tree aside from this book and [outline.md](outline.md).
- Locks in [waves.md](waves.md). First layer types only: scalars, structs, fixed arrays, slices as **offset + length**, named regions with size and alignment.
- Host OS: Windows and Linux, heap backing only.

### Output

- A Rust crate (name `stardust` unless Developer says otherwise at the gate) with:
  - IR builder for the first-layer layouts.
  - Heap-backed named region: allocate, write bytes, retrieve by path/offset, typed view on the same bytes.
  - Dump and reload of IR such that reload equals the dumped IR.
- Tests and a small demo program that only exercises layout (no foreign function).

### Benchmark

All of the following must pass on **Windows and Linux** (run the same test command on each, or CI plus local Windows):

1. **Round-trip IR:** dump then reload; the reloaded IR matches the original (equality of the IR value, not “file exists”).
2. **Store/retrieve:** a struct containing scalars, a fixed array, and a slice (`offset`, `length`) written into a heap region; retrieval by field path returns the same values that were stored.
3. **View identity:** a typed view over that region reads the same bytes as retrieval (no copy, no second buffer). Mutating through the view is visible to retrieval and vice versa.
4. **Alignment:** allocating a region with an explicit alignment yields a base meeting that alignment.
5. **Matched set:** these named tests all pass; do not replace a failing test with a different one to keep a count stable.

Exact `cargo test` invocation and crate layout are L1 implementation details; the five checks above are the contract.

### Review gate

Answered:

1. IR dump format: **JSON** (serde; IR `PartialEq` after parse).
2. Crate name: **`stardust`**.

### Out of scope

C loader, `dlopen`, mmap, file-backed regions, page control, kernel module, layout DSL, Python, LLVM, adopting foreign allocations, concurrency, IR versioning, multi-region calls.

### Zero-regression

None (first wave). After L1, this benchmark set is the baseline named **L1: 5/5** (round-trip, store/retrieve, view identity, alignment, matched set). Do not drop or swap cases without Developer approval of expectation changes.

---

## Wave L2 — layout DSL

**Was:** W6. Tests: [`tests/layout.rs`](tests/layout.rs).

**Status:** `done`

### Input

- L1–R2 done. IR is authority.
- DSL is a **thin syntax copy** of the IR: types, fields, placement, imports. Not a general-purpose language. No expressions beyond what IR already has.

### Output

- Parse a small layout language into the **same IR** the builder produces.
- Print/serialize IR back to that language (or a canonical subset) such that parse(print(ir)) equals ir for the first-layer IR used in L1–I2.
- One demo: the I2 A-then-B scenario driven by a **DSL file** for layouts and C import signatures, not only the Rust builder.

### Benchmark

1. **L1–R2** matched sets still pass.
2. **IR equality:** for a fixed first-layer IR sample (struct + slice + one C import signature), `parse(print(ir)) == ir`.
3. **Builder ≡ file:** the I2 A-then-B benchmark also passes when layouts/imports are loaded from the DSL file (same region behavior, not a different ABI).
4. Matched set: (2) and (3); do not weaken (2) to “parses without error.”

### Review gate

Answered:

- File extension `.sd`.
- Sugar (not offsets): `endian`, `layout Name repr_c { fields }`, `import Name(args) -> ret { symbol ... }`. Parser always runs `repr_c()`. Nested structs are inline `struct repr_c { ... }` (no named type refs in field position). `Ir::to_sd` succeeds only if every struct equals `repr_c()` of its fields; otherwise `Error::Dsl` (JSON dump still works).

### Out of scope

Python, LLVM, kernel module, new layout features (columnar, graphs, bit-pack), macros that generate Rust.

### Zero-regression

**L1–R2** matched sets. IR remains authority if syntax and IR ever disagree.

---

## Wave L3 — Columnar SoA (G3)

**Was:** W12. Tests: [`tests/layout.rs`](tests/layout.rs).

**Status:** `done`

### Input

- L1–I6 done. Dumpable IR is authority; `.sd` is a copy.
- Overlay on the **same** regions. First-layer POD + var-len (AoS arrays of structs) stay the default.

### Output

- Dumpable SoA IR: named columns of L1 cell types, dense (no nulls). One region: `u64` row count, then each column as a contiguous array with alignment padding. JSON round-trip is IR equality.
- `.sd` sugar in this wave: `layout Name columnar(N) { fields }`. `Ir::from_sd` / `to_sd`; if printer and IR disagree, fix the printer.
- Retrieve: row count and column by name. `ColumnView` is a typed slice on those bytes (no copy). AoS arrays of structs remain L1; this wave does not implement a second AoS field-stride view.

### Benchmark

1. **L1–I6** named sets still pass. First-layer POD + var-len still the default (existing AoS / `repr_c` cases unchanged).
2. **`columnar_ir`:** dump then reload JSON; `from_sd(to_sd(ir)) == ir` for a columnar layout (PartialEq, not string equality).
3. **`soa_view`:** store N rows; retrieve count and a named column; typed view over that column reads the same bytes as retrieval; mutate through the view is visible to retrieval (no second buffer).

### Review gate

Answered:

1. StarDust-only. Not Arrow-compatible. Not an Arrow subset. No Arrow types in dumpable IR.
2. Dense / non-null only. No validity bitmaps.
3. SoA overlay (contiguous columns). AoS stays L1.

### Out of scope

Arrow (full or subset), validity bitmaps, SoA+AoS dual implementation in this wave, language-native columnar export.

### Zero-regression

**L1–I6** named sets. First-layer POD + var-len remains the default. Do not drop or swap cases.

---

## Wave L4 — Arena / graph (G4)

**Was:** W13. Tests: [`tests/layout.rs`](tests/layout.rs).

**Status:** `done`

### Input

- L3 done. First-layer offset-in-region rule unchanged.
- Linked structures as arenas of first-layer objects. Stored truth is **offsets**, not process pointers.

### Output

- Dumpable IR type `link`: `u64` byte offset from region base; `0` = null. `.sd` names that type `link` (token mechanical at execute). JSON + `.sd` round-trip in this wave.
- Host bump-allocates objects (L1 layouts) in the region. C does not malloc/realloc/free arena memory. Cycles allowed. No GC.
- Call boundary: region bytes stay offsets. C gets `base` as `Ptr`; C does `base+offset` to walk. C must not persist absolute pointers. No in-place rewrite of links to process pointers.

### Benchmark

1. **L1–L3** named sets still pass. Offset-in-region for the first layer unchanged.
2. **`offset_cycle`:** a cycle of `link` fields round-trips through JSON and `.sd`; retrieve returns the same offsets; `0` is null.
3. **`arena_c_walk`:** host bump-allocates at least two objects with a link; region-compatible C walks via `base+offset` and mutates in place; stored links are still offsets after the call (`as_ptr()` identity; no rewrite to process pointers).

### Review gate

Answered:

1. Cycles allowed. No GC. Caller owns liveness.
2. Host bump-allocates. C borrows; C does not malloc/realloc/free.
3. Stored links stay offsets. C gets `base` and computes `base+offset`. No rewrite around the call.

### Out of scope

GC, free-list, C bump cursor during the call, general heaps, rewriting offsets to absolute pointers.

### Zero-regression

**L1–L3** named sets. Offset-in-region remains for the first layer. Do not drop or swap cases.

---

## Wave L5 — Bit-pack overlay (G5)

**Was:** W14. Tests: [`tests/layout.rs`](tests/layout.rs).

**Status:** `done`

### Input

- L4 done. Byte-aligned first layer unchanged.
- Bit-level field placement is an **overlay**, not the default layout. Byte endianness (L1) and bit order are independent IR fields.

### Output

- IR records `bit_order`: `lsb0 | msb0` (alongside byte `endianness`, not derived from it). This wave implements **`lsb0` only**; `msb0` access is `Err` (L1-shaped). JSON + `.sd` round-trip.
- Overlay on a L1 integer cell (`u8` / `u16` / `u32` / `u64`). Fields have bit widths and **must fit** in that container (no spanning). `.sd`: `bit_order` on the IR plus `bitpack T { field: width, ... }` on a L1 cell (tokens mechanical at execute).
- Host `View` get/set of a bit-field is a scalar in place (typed view, no second buffer). Foreign calls still receive packed `Ptr` / `Len`; they unpack themselves or use C. No unpack trampoline into Python/Haskell/Lean objects.

### Benchmark

1. **L1–L4** named sets still pass. Byte-aligned first layer unchanged.
2. **`bitpack_ir`:** dump then reload; `from_sd(to_sd(ir)) == ir` for a `lsb0` bitpack on a L1 cell. Declaring or accessing `msb0` returns `Err` (not a silent guess).
3. **`bitpack_view`:** get/set a bit-field through `View`; retrieval sees the same packed bytes; a region-compatible C call on the container span sees packed bytes (not unpacked scalars). No second buffer.

### Review gate

Answered:

1. Record `lsb0 | msb0` in IR. Implement `lsb0` only; `msb0` is `Err`. Not derived from byte endianness.
2. Host `View` get/set in place. Foreign calls get packed `Ptr`/`Len`. Not C-only (no View). Not an unpack trampoline.
3. L1 container only (fields fit; no spanning). Free bitstream is parked.

### Out of scope

Spanning bitstream (parked), unpack trampoline, C-only overlay with no View, implementing `msb0` access.

### Zero-regression

**L1–L4** named sets. Byte-aligned first layer unchanged. Do not drop or swap cases.

---

## Wave L6 — Free bitstream (H10)

Tests: [`tests/layout.rs`](tests/layout.rs) (`bitstream_ir`, `bitstream_view`).

**Status:** `done`

### Input

- L5 done. Byte-aligned first layer unchanged. `Type::Bitpack` + `bit_offset` / `width`.
- Not a second packer. Not **O1**.

### Output

- `BitpackType.container: Option<Scalar>`; add `span: u64`. L5: `container = Some(u8|u16|u32|u64)`, `span` omitted in JSON means container size; fields still fit the cell. L6: `container = None`, `span` is the byte length; fields may cross bytes; total bits ≤ `span * 8`; each `width` ≤ 64.
- Sequential `bit_offset` from 0 (same as L5). `msb0` access still `Err`. Nested bitpack in `repr_c` still `Err`.
- Host `View` get/set is `Value::U64` in place. Foreign calls still packed `Ptr`/`Len`.
- `.sd`: `layout Frame bitpack span 5 { a: 12; b: 20; }`. JSON round-trip IR equality. Old L5 JSON without `span` still parses.

### Benchmark

1. **Zero-regression (matched sets, not counts):** L1 `round_trip_ir`, `store_retrieve`, `view_identity`, `alignment`; L2 `ir_equality`, `builder_eq_file`; L3 `columnar_ir`, `soa_view`; L4 `offset_cycle`, `arena_c_walk`; L5 `bitpack_ir`, `bitpack_view`; I1 `in_place_c`, `reject_incompatible`; I2 `a_then_b`, `same_backing`; R1 `mmap_c`, `file_round_trip`; R2 `pin`, `advice`, `huge`, `numa`, `pin_mmap_c`; O1 `scalar_arg`, `scalar_ret`; O2 `class_ir`; O3 `class_c_method`; O4 `prefix_ir`, `prefix_override`.
2. **Named L6:** `bitstream_ir` ([`tests/layout.rs`](tests/layout.rs)) — dump/reload span-only bitpack; `.sd` `bitpack span N`; mix `bitpack u32 span N` is `Err`; field wider than 64 is `Err`; `msb0` still `Err`. `bitstream_view` — get/set a field that crosses a byte boundary; packed bytes match; a region-compatible C call on the span sees packed bytes.

### Review gate

Answered:

1. Optional container (`None` + `span` for bitstream; L5 `Some(u8|u16|u32|u64)` unchanged).
2. Same packer (not a second packer; not **O1**).
3. Scalar fields `width` ≤ 64.
4. `.sd`: `bitpack span N`.

### Out of scope

Second `Type`, `msb0`, unpack trampoline, nested bitpack, fields > 64 bits, **O1** trampoline changes.

### Zero-regression

Userspace named sets in Benchmark (1). L5 container form unchanged. Do not drop or swap cases.

---

## Done — Import

Former waves grouped here. Status `done`. Do not re-execute.

## Wave I1 — C loader, one call

**Was:** W2. Tests: [`tests/import.rs`](tests/import.rs).

**Status:** `done`

### Input

- L1 done; baseline **L1: 5/5**.
- Existing C, region-compatible only; offsets in region, pointers at the call; StarDust or caller allocates; C borrows; never silent copy; reject incompatible signatures.

### Output

- Load an existing compiled C artifact (`.dll` on Windows, `.so` on Linux).
- Declare a region-compatible signature in IR (or builder) and bind it to a symbol.
- Call that function with `base + offset` (and length) into a live region so it mutates or reads **in place**.
- Import of a **non**-region-compatible signature fails with an error (no call, no silent copy).
- A tiny C test library in-tree (or equivalent) used only for the benchmark, with a function of the shape `process(uint8_t *p, size_t n)` (or documented equivalent).

### Benchmark

1. **L1: 5/5** still passes (same cases).
2. **In-place C:** the test C function runs; bytes in the region after the call match what the C function is specified to write; the pointer passed is inside the region (not a copied buffer).
3. **Reject incompatible:** at least one documented incompatible signature (e.g. callee-owned `malloc` return, or embedded raw pointer) fails import; no call occurs.
4. Matched set: (2) and (3) both pass; do not swap the reject case for a weaker check.

### Review gate

None beyond confirming the in-tree C test function’s exact symbol and signature in the implementation notes when coding (mechanical, not a product fork). If a new product choice appears (e.g. which calling convention beyond the platform C ABI), stop and ask.

### Out of scope

Second C function in sequence (that is I2), mmap, DSL, other languages, adopt-`malloc`, trampolines that copy, kernel module.

### Zero-regression

**L1: 5/5** must still pass with the same cases.

---

## Wave I2 — two C functions, one region

**Was:** W3. Tests: [`tests/import.rs`](tests/import.rs).

**Status:** `done`

### Input

- I1 done; C loader works for one region-compatible call.
- North star: no pre/post-processing between functions when they share a region.

### Output

- Two region-compatible C functions (in-tree test lib is fine): **A** writes the region; **B** reads or further mutates it.
- Host sequence: allocate region → call A → call B with **no** copy, **no** re-encode, **no** second region, **no** Rust struct round-trip of the payload.
- Documented API for “call these two imports on this region” sufficient for the demo (no extra language).

### Benchmark

1. **L1: 5/5** and **I1** in-place + reject still pass (matched sets).
2. **A-then-B:** after A then B, region bytes match the composition of A and B’s specified effects.
3. **Same backing:** A and B receive pointers into the **same** region allocation (same base); tests must show it is not two copies (e.g. same base address, or a sentinel write by A seen by B without host memcpy of the payload).
4. Matched set: do not replace (3) with a weaker “both functions ran” check.

### Review gate

None locked. Stop if the only way to pass is a hidden copy.

### Out of scope

Python, LLVM, mmap, DSL, kernel, multi-region, concurrent calls.

### Zero-regression

**L1: 5/5** and **I1** (in-place C + reject incompatible).

This wave **closes the first layer** (POD + var-len regions + existing C, zero-copy when compatible).

---

## Wave I3 — Python

**Was:** W7. Tests: [`tests/python.rs`](tests/python.rs) (feature `python`).

**Status:** `done`

### Input

- First layer + DSL done (L1–L2).
- Goal: Python and C see the **same region** without rewriting the payload. Representation stays typed views / buffers, not “make a Python object” by default.

### Output

Depends on the mechanism lock. After the gate is answered: one loader path such that a region-compatible Python callable (or buffer consumer) and a C function can operate on one region with no payload copy.

### Benchmark (after the gate)

1. **L1–L2** matched sets still pass.
2. **Shared region:** Python reads or writes bytes that C also sees in the same region (same backing; no host re-encode of the payload).
3. Matched set: do not replace (2) with “Python ran in a subprocess and got a copy.”

### Review gate

Answered:

1. Loader mechanism: embed CPython via PyO3 in-process (cargo feature `python`, `auto-initialize`).
2. Python-facing handle: writable memoryview over `[region.base+offset, length)` (`PyBUF_WRITE`). Not PyCapsule, not a `bytes`/`bytearray` copy.
3. Windows and Linux in this wave. Missing libpython is a real failure.

### Out of scope

Haskell, LEAN, LLVM, kernel module, native `PyObject` as default representation, silent copies.

### Zero-regression

**L1–L2** matched sets.

---

## Wave I4 — LLVM

**Was:** W8. Tests: [`tests/llvm.rs`](tests/llvm.rs) (feature `llvm`).

**Status:** `done`

### Input

- I3 done (or Developer explicitly reorders; default is after Python).
- Import LLVM-side code **without translating it into Rust syntax**. Target is a function that can use a region (pointers + lengths or IR-declared layout).

### Output

- Compile in-tree LLVM 14-style textual IR (`.ll`, `i8*` not opaque `ptr`) with `clang -shared -fPIC` to a platform `.dll`/`.so`.
- Reuse I1 `Library::load` / `bind` / `ImportFn::call`. Shape `(Ptr, Len) → Void`. Pointer is `region.as_mut_ptr().add(offset)` — no staging copy.
- No LLVM in `stardust` (no inkwell, llvm-sys, JIT, `lli`). Cargo feature `llvm` pulls the fixture and gates the test. Default `cargo test` stays gcc-only (L1–L2 + I1 C). Windows and Linux.

### Benchmark (after the gate)

1. Prior matched sets still pass (L1–I3).
2. **LLVM on region:** the LLVM-side function’s specified effect is visible in the region bytes; pointer argument is in-region, not a copied staging buffer.
3. Matched set: do not replace (2) with “we compiled LLVM to a .so in a separate undocumented step” unless that *is* the locked mechanism.

### Review gate

Answered:

1. Mechanism: compile in-tree `.ll` to a platform `.dll`/`.so`, then reuse I1 `Library::load` / `bind` / `ImportFn::call`. Same shape `(Ptr, Len) → Void`. Pointer is `region.as_mut_ptr().add(offset)` — no staging copy. No LLVM in `stardust`.
2. IR dialect: textual `.ll`, LLVM 14-compatible (`i8*`, not opaque `ptr`). Fixture finds `clang` on PATH (not the I1 `cc` gcc). Feature `llvm`. Windows and Linux this wave. Missing clang is a real failure (panic, never skip).

### Out of scope

Haskell, LEAN, kernel module, writing a C compiler, source ingest of C/Python.

### Zero-regression

**L1–I3** matched sets.

---

## Wave I5 — Haskell (G1)

**Was:** W10. Tests: [`tests/haskell.rs`](tests/haskell.rs) (feature `haskell`).

**Status:** `done`

### Input

- L1–R3 done. Baseline named sets below. I1 call engine is `(Ptr, Len) → Void` only.
- GHC on PATH. Import Haskell **without translating it into Rust**. Reuse I1 `Library::load` / `bind` / `ImportFn::call`.
- Windows and Linux. MSVC/`link.exe` unsupported.

### Output

- Cargo feature `haskell`. In-tree fixture crate (`publish = false`) compiled with GHC `foreign export ccall` of shape `(Ptr, Len) → IO ()` to a platform `.dll`/`.so`. Default `cargo test` stays gcc-only (does not need GHC).
- Bind and call that symbol through I1. Haskell sees a borrowed `Ptr` into the live region (not `ForeignPtr`, not pinned `ByteArray#`). Pointer is `region.as_mut_ptr().add(offset)` — no staging copy. No Haskell types in dumpable IR.
- North star: C then Haskell on one `Region`; `as_ptr()` identity; host must not retrieve/store/view/memcpy the payload between calls.
- Missing GHC (or fixture compile failure) with `--features haskell` **panics**, never skips.

### Benchmark

1. **L1–R3** named sets still pass **without** feature `haskell`: **L1** `round_trip_ir`, `store_retrieve`, `view_identity`, `alignment`; **I1** `in_place_c`, `reject_incompatible`; **I2** `a_then_b`, `same_backing`; **R1** `mmap_c`, `file_round_trip`; **R2** `pin`, `advice`, `huge`, `numa`, `pin_mmap_c`; **L2** `ir_equality`, `builder_eq_file`; **I3** `shared_region`; **I4** `llvm_on_region`; **R3** `module_backed` (skip if no device).
2. **`--features haskell`:** named test **`haskell_on_region`**: C `write_a` fills a span 0xA1, Haskell mutates it in place (wrapping +1), then the span is 0xA2; `as_ptr()` identity through both calls; prefix/suffix unchanged. Do not replace this with “we compiled Haskell in a separate undocumented step” unless that *is* the locked GHC → `.dll`/`.so` → I1 path.

### Review gate

Answered:

1. Mechanism: GHC `foreign export ccall` → platform `.dll`/`.so` → I1. No RTS embed in `stardust`. Not out-of-process.
2. Region view: borrowed `Ptr`. Not `ForeignPtr`. Not pinned `ByteArray#`.
3. Toolchain: I4-shaped. Feature `haskell`; missing GHC with the feature panics, never skips. Default `cargo test` stays gcc-only.

### Out of scope

RTS embed, `ForeignPtr`, `ByteArray#`, Haskell-as-process, Haskell types in dumpable IR, scalar/POD trampolines, MSVC.

### Zero-regression

**L1–R3** named sets without feature `haskell`. Do not drop or swap cases.

---

## Wave I6 — Lean (G2)

**Was:** W11. Tests: [`tests/lean.rs`](tests/lean.rs) (feature `lean`).

**Status:** `done`

### Input

- I5 done (or Developer explicitly reorders; default is after Haskell).
- Lean 4 on PATH plus the same gcc/clang as I1 (Lean 4 emits C, then a platform `.dll`/`.so`). Import Lean **without translating it into Rust**. Reuse I1.
- Windows and Linux. MSVC/`link.exe` unsupported.

### Output

- Cargo feature `lean`. In-tree fixture crate (`publish = false`) built from Lean 4 to a platform `.dll`/`.so` exporting a `(Ptr, Len) → Void` C ABI. Default `cargo test` stays gcc-only (does not need Lean).
- Bind and call through I1. Lean sees a borrowed `USize` / FFI pointer into the live region, not a Lean `ByteArray` (that copies). No staging copy. No Lean types in dumpable IR.
- North star: C then Lean on one `Region`; `as_ptr()` identity; host must not retrieve/store/view/memcpy the payload between calls.
- Missing Lean 4 or C toolchain with `--features lean` **panics**, never skips. Not Lean 3.

### Benchmark

1. **L1–I5** named sets still pass **without** feature `lean` (I5’s `haskell_on_region` only when `--features haskell`).
2. **`--features lean`:** named test **`lean_on_region`**: C `write_a` fills a span 0xA1, Lean mutates it in place (wrapping +1), then the span is 0xA2; `as_ptr()` identity through both calls; prefix/suffix unchanged. Do not replace this with “we compiled Lean in a separate undocumented step” unless that *is* the locked Lean 4 → C → `.dll`/`.so` → I1 path.

### Review gate

Answered:

1. Mechanism: Lean 4 compile → C → platform `.dll`/`.so` → I1. No Lean runtime embed in `stardust`. Not out-of-process.
2. Region view: borrowed `USize` / FFI pointer. Not `ByteArray`.
3. Toolchain: I4-shaped. Feature `lean`; missing toolchain with the feature panics, never skips.

### Out of scope

Embed Lean runtime, `ByteArray` as the region view, Lean-as-process, Lean types in dumpable IR, Lean 3, scalar/POD trampolines, MSVC.

### Zero-regression

**L1–I5** named sets. Do not drop or swap cases.

---

## Wave I7 — Multi-region calls (H8)

Tests: [`tests/import.rs`](tests/import.rs) (`two_region_c`). Fixture crate: [`crates/stardust-c`](crates/stardust-c).

**Status:** `done`

### Input

- **O1** done. **L6** done (book order). `ImportFn::call(&mut Region, args)` one-region. Exclusive single-thread borrow.

### Output

- Host `CallArg::Ptr { region: usize, offset }`. Dumpable `Param::Ptr` unchanged (no region names).
- Keep `call(&mut Region, args)`: every `Ptr.region` must be `0` else `TypeMismatch`. Add `call_regions(regions: &mut [&mut Region], args)`. Index ≥ len is `OutOfBounds`. Span past that region’s size stays `OutOfBounds`. `OwnedPtr` still `IncompatibleImport` before dlsym. `call` is the one-region path (`call_regions` on a single-element slice after the region-0 check).
- Two offsets in one region: use `call` (both `region: 0`), not two slice entries for the same object.
- C fixture in [`crates/stardust-c`](crates/stardust-c): `pair_write(uint8_t *a, size_t an, uint8_t *b, size_t bn)` fills `a` with 0xA1 and `b` with 0xB2. Kept `process` / `write_a` / `mut_b` / `add_u32` / `load_u32` / `arena_walk`. `BoundClass::call` / `call_python` stay one-region **E2a** (`CallArg::Ptr { region: 0, offset }`).

### Benchmark

1. **Zero-regression (matched sets, not counts):** L1 `round_trip_ir`, `store_retrieve`, `view_identity`, `alignment`; L2 `ir_equality`, `builder_eq_file`; L3 `columnar_ir`, `soa_view`; L4 `offset_cycle`, `arena_c_walk`; L5 `bitpack_ir`, `bitpack_view`; L6 `bitstream_ir`, `bitstream_view`; I1 `in_place_c`, `reject_incompatible`; I2 `a_then_b`, `same_backing`; R1 `mmap_c`, `file_round_trip`; R2 `pin`, `advice`, `huge`, `numa`, `pin_mmap_c`; O1 `scalar_arg`, `scalar_ret`; O2 `class_ir`; O3 `class_c_method`; O4 `prefix_ir`, `prefix_override`. One-region `call` / `(Ptr, Len) → Void` / O1 scalars unchanged.
2. **Named I7:** `two_region_c` ([`tests/import.rs`](tests/import.rs)) — two heap regions (size 32, span offset 8, len 16); C writes both in one `call_regions`; region A span is 0xA1, region B span is 0xB2; prefix/suffix unchanged; `as_ptr()` identity on each; `call` with `region != 0` is `TypeMismatch`; one-region `process` via `call` with `region: 0` still wrapping +1; `call_regions` with `region: 2` on a 2-slice is `OutOfBounds`.

### Review gate

Answered:

1. Host region index on `CallArg::Ptr` (dumpable `Param::Ptr` unchanged).
2. `ImportFn` only (`BoundClass` still one region / **E2a**).
3. Keep `call` + add `call_regions`.

### Out of scope

`BoundClass` extra regions, Python multi-buffer, `Param::Struct`, H9 copy/adopt, named regions in IR, concurrency.

### Zero-regression

Named sets in Benchmark (1). Do not drop or swap cases.

---

## Done — Region and kernel

Former waves grouped here. Status `done`. Do not re-execute.

## Wave R1 — mmap and file-backed regions

**Was:** W4. Tests: [`tests/region.rs`](tests/region.rs).

**Status:** `done`

### Input

- I2 done; first-layer data plane stable.
- Same region API; heap remains valid. mmap and file-backed are additional backings, not a second library.

### Output

- Region create/open options: heap (existing), anonymous mmap, file-backed mmap (or equivalent on Windows: file mapping).
- Retrieve, typed views, and I1/I2 C calls work on mmap-backed regions the same as heap (offsets stored, pointers at C boundary).
- File-backed: write region, drop mapping, reopen file, retrieve the same stored bytes (for contents that were written to the file).

### Benchmark

1. Prior matched sets: **L1: 5/5**, **I1** in-place + reject, **I2** A-then-B on **heap**.
2. **mmap C:** A-then-B (or at least one in-place C call plus retrieve) on an anonymous mmap region; same-backing rule as I2.
3. **File round-trip:** file-backed region bytes survive close and reopen; retrieved fields match what was stored.
4. Matched set: (2) and (3) both pass on Windows and Linux.

### Review gate

None. If Windows file-mapping vs Linux mmap forces an API split visible to users, stop and ask (the lock is **one region API**).

### Out of scope

`mlock` / `madvise` / huge pages / NUMA (R2), kernel module, DSL, other languages, shared memory across processes (gated).

### Zero-regression

**L1: 5/5**, **I1**, **I2** on heap.

---

## Wave R2 — userspace page control

**Was:** W5. Tests: [`tests/region.rs`](tests/region.rs).

**Status:** `done`

### Input

- R1 done; mmap/file-backed exist.
- These are **syscall wrappers** as explicit region options, not a driver.

### Output

- Region options (or equivalent APIs) for:
  - pin / `mlock` (Windows: `VirtualLock` or documented equivalent),
  - `madvise`-style advice (Linux; Windows equivalent or a documented no-op with a test that the option is accepted or rejected honestly),
  - huge pages (Linux `MAP_HUGETLB` / `madvise` huge; Windows large pages if available — if unavailable, fail clearly, do not pretend),
  - NUMA bind (Linux; Windows: honest unsupported or equivalent).
- Applying an option either takes effect in a way a test can observe **or** returns a distinct error. No silent ignore except where the gate below allows platform no-ops.

### Benchmark

1. Prior matched sets still pass on heap and mmap (L1–R1).
2. **Pin:** a pinned region’s lock API reports success on at least one OS; pages are not required to be proven unevictable in CI, but `mlock`/`VirtualLock` must be invoked and succeed or fail with a real error (not a stub `Ok`).
3. **Advice / huge / NUMA:** each option has a test: success path **or** documented platform-unsupported error. Matched set of four option families (pin, advice, huge, NUMA); do not drop a family.
4. C in-place call still works on a mmap region that also requested pin (where pin succeeded).

### Review gate

Answered:

1. **Pin** is real on both OS: Windows `VirtualLock`, Linux `mlock`. Never stub `Ok`. `pin` is never `Unsupported`.
2. **Advice / huge / NUMA** on Windows are distinct `Error::Unsupported` (Display names the family: advice / huge / numa), not a silent no-op. Linux implements them (`madvise`; huge via `madvise(MADV_HUGEPAGE)` after map, not `MAP_HUGETLB`; NUMA `mbind` `MPOL_BIND`). Same public names on both OS.

### Out of scope

Kernel module (R3), Windows driver, layout DSL, other languages, custom page-table manipulation, kernel allocators.

### Zero-regression

**L1–R1** matched sets (heap + mmap + file round-trip + C A-then-B).

---

## Wave R3 — Linux kernel module (region backing)

**Was:** W9. Tests: [`tests/linux_module.rs`](tests/linux_module.rs) (feature `kernel`).

**Status:** `done`

### Input

- Userspace region API exists (heap, mmap, page options).
- Module job: **allocate / pin / map named regions into processes**. Not FFI, not layout validation, not in-kernel foreign calls.
- Windows keeps userspace backings only in this wave.

### Output

- A Linux kernel module (or equivalent in-tree driver) that provides region backing.
- Userspace StarDust creates/maps a region whose pages are owned/backed by that module, using the **same region API** as L1/R1 (retrieve, views, region-compatible C call).
- Document how to build and load the module; tests that need the module are skipped or fail clearly when not loaded (do not fake success on Windows).

**Build and load** (root for `insmod`/`rmmod` only; `/dev/stardust` is 0666 afterward):

```
make -C kmod/stardust
sudo insmod kmod/stardust/stardust.ko
sudo rmmod stardust
```

`KDIR` defaults to `/lib/modules/$(uname -r)/build`. Empty Cargo feature `kernel` gates `tests/linux_module.rs`; default `cargo test` never needs the module.

### Benchmark

1. **L1–I4** userspace matched sets still pass on Linux and Windows **without** the module.
2. **On Linux with module loaded:**
   - Allocate/map a named region via the module backing.
   - Store/retrieve and a typed view work (L1-style).
   - At least one region-compatible C in-place call works on that region (I1-style).
3. Matched set: (2) includes map + retrieve + C; do not ship map-only.

### Review gate

Answered:

1. Userspace surface: **`/dev/stardust` char device + `mmap`**. Not ioctl-only, not a custom filesystem, not `open_file` on a fake path.
2. Identity: ioctl **`CREATE(name, size, align)`** on that fd allocates pages keyed by a **unique string name**; **`mmap` on the same fd** maps those pages. Duplicate name is an error. `Region::name` equals the kernel name. R3 is one process; R5 attach-by-name is later.
3. Privileges: **`insmod`/`rmmod` need root**. After load, `/dev/stardust` is **0666** (ioctl/mmap do not need root). Unsigned module, built against the **running** kernel (no Secure Boot/signing this wave). Feature `kernel`. CI: `sudo insmod` then `cargo test --features kernel`.

### Out of scope

Windows driver (R4), cross-process share (R5), layout enforcement in kernel, putting loaders in kernel, custom filesystems unless required by the locked surface.

### Zero-regression

Userspace **L1–I4** matched sets without the module.

---

## Wave R4 — Windows KMDF (G6)

**Was:** W15. Tests: [`tests/kmdf.rs`](tests/kmdf.rs) (feature `kernel`).

**Status:** `done`

### Input

- L5 done. R3 Linux surface is the contract to match: `CREATE(name, size, align)`, map the same handle, last close frees, no DESTROY, `Region::name` equals the kernel name.
- Public API stays `Region::map_module`. No OS types in the public API.
- Windows without a driver: `map_module` remains `Error::MapFailed`, never silent `Ok`.

### Output

- KMDF (WDF kernel-mode) driver providing kernel-owned region backing. Device `\\.\stardust` (or documented equivalent). IOCTL `CREATE(name, size, align)`; map the same handle; last close frees name and pages; no DESTROY. IOCTL numbers may differ from Linux; CREATE fields mean the same as [`kmod/stardust/stardust_uapi.h`](kmod/stardust/stardust_uapi.h).
- Userspace `Region::map_module` succeeds on Windows when the device is present. Same `as_ptr` / retrieve / View / C-call path as R3.
- Cargo feature `kernel` (same as R3). Default `cargo test` never builds the driver. Tests **skip** if the device is missing (including Linux hosts without this driver). If the device is present, map/retrieve/view/C must be correct or **fail**.

### Benchmark

1. **L1–L5** userspace named sets still pass on Windows and Linux **without** a Windows driver.
2. **On Windows with the device present:** named test **`kmdf_backed`**: `map_module` + store/retrieve + typed view + at least one region-compatible C in-place call. Matched set includes map + retrieve + C; do not ship map-only.
3. **Device missing:** `kmdf_backed` skips (including Linux). `map_module` on Windows without the driver is `Error::MapFailed`, never silent `Ok`.

### Review gate

Answered:

1. Driver model: KMDF (WDF kernel-mode). Not WDM. Not UMDF.
2. Same userspace contract as R3 on `\\.\stardust` (or documented equivalent): CREATE, map the same handle, last close frees, no DESTROY, `Region::name` = kernel name. `Region::map_module` unchanged.
3. Tests: R3-shaped. Feature `kernel`. Skip if device missing. Fail if present but wrong.

### Out of scope

UMDF, WDM, ioctl-only data path (no map), ATTACH (that is R5), layout enforcement in kernel, putting loaders in kernel.

### Zero-regression

**L1–L5** userspace named sets without a Windows driver. Linux R3 `module_backed` unchanged. Do not drop or swap cases.

---

## Wave R5 — Kernel attach-by-name (G7)

**Was:** W16. Tests: [`tests/attach.rs`](tests/attach.rs) (feature `kernel`).

**Status:** `done`

### Input

- R4 done (KMDF exists; Linux R3 exists). R3/R4 are one process until this wave.
- Goal: two processes map the **same named region**; offsets remain valid; each process’s C gets `base + offset` for **that** process’s mapping.

### Output

- Kernel `ATTACH(name)` (or open-by-name) on `/dev/stardust` and `\\.\stardust`. ATTACH increments a refcount; last close **across all processes** frees name and pages. No DESTROY. Creator exit does not free if another process still holds the mapping.
- Device stays **0666**. Any process that knows the name may ATTACH. Name is the capability. No per-region mode/ACL in this wave.
- Userspace API: attach/map by existing kernel name (same `Region` byte API). Not userspace `MAP_SHARED` / named file mapping as the share mechanism.
- Tests: feature `kernel`. Default `cargo test` never requires the driver. **Skip** if the device is missing. If present, ATTACH + two-process map/retrieve/C/view must be correct or **fail**.

### Benchmark

1. **L1–R4** named sets still pass. Single-process R3/R4 semantics unchanged when only one process holds the name (last close still frees).
2. **`attach_two_process`:** process A `CREATE`s (or `map_module`); process B `ATTACH`es the same name; offsets stored in A are valid in B; C in each process mutates via that process’s `base+offset`; `as_ptr()` is per mapping (not the same VA). Device missing → skip.

### Review gate

Answered:

1. Kernel attach-by-name on Linux R3 **and** R4 KMDF. Not userspace `MAP_SHARED` / named file mapping. Not both in this wave.
2. Lifetime: refcounted last close across all processes. No DESTROY. Creator exit does not free if others still hold.
3. Permissions: device 0666; knowing the name is enough to ATTACH. Not a security boundary. No ACL/mode in this wave.

### Out of scope

Userspace `MAP_SHARED` / named file mapping as the share mechanism, DESTROY, per-region ACLs/modes, IPC/sockets-as-data-plane.

### Zero-regression

**L1–R4** named sets. Single-process region semantics unchanged. Do not drop or swap cases.

---

## Wave R6 — Slot allocator (F3b)

Tests: [`tests/region.rs`](tests/region.rs) (`slot_reuse`). Host: [`src/slot.rs`](src/slot.rs) (`SlotAlloc`).

**Status:** `done`

### Input

- L4 `Bump` unchanged (no free, cursor not stored). F1a/F2a as amended. **L6** and **I7** done (book order).

### Output

- Host `SlotAlloc` (new type, not IR): `new(&mut Region)`, `alloc(&Type) -> offset` (same placement rules as `Bump`: offset `0` never allocated; first candidate `8`; `align_up`), `free(offset)` using a host map of live `(offset, size)`. Unknown or double-free `Err`. Does not write a free-list into the region.
- Reuse after free allowed. View/call do not consult `SlotAlloc`. Mixing `Bump` + `SlotAlloc` on one region is caller error (not checked).
- Export from the crate root next to `Bump`. No class `place` on `SlotAlloc` (that is not **O5**).

### Benchmark

1. **Zero-regression:** I7 set plus `two_region_c`. L4 `offset_cycle`, `arena_c_walk` unchanged.
2. **Named R6:** `slot_reuse` ([`tests/region.rs`](tests/region.rs)) — alloc A, alloc B, free A, alloc C (C may equal A’s offset); write C; B’s bytes unchanged; double-free `Err`; C on the live slot is in-place (`as_ptr()` identity).

### Review gate

Answered:

1. Host-only (`SlotAlloc` is not IR).
2. `free(offset)` + host map of live `(offset, size)` (no in-region free-list).
3. F1a reuse after free allowed.

### Out of scope

In-region free-list overlay, dumpable IR, generation counters, mix checking, `BoundClass::place` on `SlotAlloc`, dtors.

### Zero-regression

Named sets in Benchmark (1). Do not drop or swap cases.

---

## Done — Call engine

## Wave O1 — Scalar trampolines

Tests: [`tests/import.rs`](tests/import.rs) (`scalar_arg`, `scalar_ret`). Fixture crate: [`crates/stardust-c`](crates/stardust-c).

**Status:** `done`

Prerequisite for class **call** demos (E1b). Does **not** add classes. Does **not** un-park multi-region calls. Does **not** add `Param::Struct`.

### Input

- Done book (L1–L5, I1–I6, R1–R5) complete; this archive.
- IR authority [`src/ir.rs`](src/ir.rs): `Param = Ptr | Len | Scalar | OwnedPtr`, `Ret = Void | Scalar | OwnedPtr`. No POD-struct import params. No language types in IR.
- Call engine before this wave: `OwnedPtr` → `IncompatibleImport`; anything other than `(Ptr, Len) → Void` → `UnsupportedCall`.
- Locks: region-compatible C rule; never silent copy of the region payload; exclusive single-thread borrow.
- In-tree C fixture (gcc/clang `-shared`; MSVC still parked).

### Output

- Fixture packages retitled (drop `-w*`): `stardust-c` / `stardust-ll` / `stardust-hs` / `stardust-lean` (env `STARDUST_C_LIB`, `STARDUST_LL_LIB`, `STARDUST_HS_LIB`, `STARDUST_LEAN_LIB`). Attach child env `STARDUST_W16_*` and region temp prefix `stardust-w4-*.bin` unchanged.
- `Library::bind` / `ImportFn::call` via `libffi` for any mix of existing `Param`/`Ret` except `OwnedPtr` (still `IncompatibleImport` before dlsym). Marshals `Ptr` → `region.as_mut_ptr().add(offset)` (span bounds-check when a following `Len` is present; `Ptr` alone bounds-checks offset), `Len` → `usize`, `Scalar` by value (platform C ABI). `Ret::Void` → `CallRet::Void`; `Ret::Scalar` written into host `CallRet` (not a region memcpy).
- Host API: `CallArg::{Ptr, Len, Scalar(CallScalar)}`; `ImportFn::call` → `Result<CallRet>` (`Void` | `Scalar(...)`). Arg count/types must match the bound `Import` or `TypeMismatch`.
- `(Ptr, Len) → Void` unchanged (I1/I2). Multi-region (two unrelated pointers not both `base+offset` into the same `&mut Region`) not a success path.
- C fixture grew `add_u32` (scalar-arg mutate) and `load_u32` (scalar return). Kept `process` / `write_a` / `mut_b` / `arena_walk`.
- Windows GNU: crates.io `libffi-sys` autotools path fails on this host; [`vendor/libffi-sys`](vendor/libffi-sys) compiles the same C sources with `cc` (`[patch.crates-io]`).

### Benchmark

1. **Zero-regression (default `cargo test`):** L1 `round_trip_ir`, `store_retrieve`, `view_identity`, `alignment`. I1 `in_place_c`, `reject_incompatible`. I2 `a_then_b`, `same_backing`. R1 `mmap_c`, `file_round_trip`. R2 `pin`, `advice`, `huge`, `numa`, `pin_mmap_c`. L2 `ir_equality`, `builder_eq_file`. L3 `columnar_ir`, `soa_view`. L4 `offset_cycle`, `arena_c_walk`. L5 `bitpack_ir`, `bitpack_view`. Do not drop or swap cases.
2. **Named O1 set:** `scalar_arg` (C scalar-arg mutate on a heap region, then I1 `process` on the same span, `as_ptr()` identity, `OwnedPtr` still `IncompatibleImport`); `scalar_ret` (non-void scalar return from the same region after an I1 `(Ptr, Len) → Void` call, `as_ptr()` identity).

### Review gate

Answered:

1. This wave is the trampoline prerequisite (E1b / H3 = wave). Class IR is **O2**; class **calls** are **O3**.
2. `OwnedPtr` stays incompatible. Multi-region stays parked (H8).
3. Platform C ABI for scalars; `Ptr` is `base+offset`; no staging copy of the region span. No `Param::Struct` this wave.

### Out of scope

Classes, methods, language tags, inheritance, Python trampoline, MSVC fixtures, adopt-`malloc`, multi-region calls, concurrency, `Param::Struct`, libffi as a user-facing API.

### Zero-regression

Userspace named sets listed in Benchmark (1). Feature-gated I3–I6 / R3 / R4 / R5 skip-or-fail rules unchanged (callers use `stardust_c::process_lib_path()` after the crate rename).

---

## Done — Class IR

## Wave O2 — Class IR, language tag

Tests: [`tests/layout.rs`](tests/layout.rs) (`class_ir`).

**Status:** `done`

Dumpable class surface. No method **calls** (that is **O3**). Tests do not call.

### Input

- OOP locks (A1, C1b, C2a, D2b). L2 `.sd` is a copy of IR.
- Layouts already exist (`repr_c`, columnar, `link`/arena, bitpack). A class names a layout and a method table; object bytes stay that layout (no vtable in the region).

### Output

- Dumpable IR: named **class** = layout reference + method table (each method is an import: symbol, args, ret, **language/loader tag**). Default kind is the named layout’s `Type` (not a class overlay field). JSON round-trip is IR equality (`#[serde(default)]` so dumps without `classes` / `lang` still parse; missing tag is `Lang::C`).
- Types: `Lang` (`C` | `Python` | `Llvm` | `Haskell` | `Lean`), `Class { layout, methods }`, `Import.lang`, `Ir.classes`. `Ir::insert_class` / `Class::insert_method`. Duplicate class → `DuplicateClass`; duplicate method → `DuplicateMethod`; unknown layout ref → `UnknownLayout` (including `from_json` / `from_sd`).
- `.sd` prints/parses that IR. Print iff the class IR is what a fresh builder would produce (same discipline as `repr_c` / columnar / bitpack). Class before layout in a file is allowed; dangling layout name is `Err`.
- Class does not store backing. Language tag is metadata; still no `PyObject` / Haskell / Lean types in IR.

### Benchmark

1. **Zero-regression:** userspace named sets (L1–L5, I1–I2, R1–R2, O1). I1 import records without a language tag still parse (default `C`). Do not drop or swap cases.
2. **Named O2 set:** `class_ir`: dump/reload class+methods+tags (kind implied by named layout); `.sd` round-trip equals IR; missing layout / duplicate names are `Err`.

### Review gate

Answered:

1. A1: class in IR/`.sd`; bytes data-only.
2. C1b: no overlay field on the class; kind is the named layout’s `Type`. C2a: backing not in IR; instance overlay override is host-side (**O3**), not required to dump. Arena is not an overlay; bump-vs-ordinary allocate is an **O3** host flag.
3. D2b: closed `Lang` enum on method/import. Default `C`. Not a language-native type.
4. **`.sd` grammar:**

```text
class Widget {
  layout Packet;
  method bump(ptr, len) -> void {
    symbol bump_u8;
    lang c;
  }
  method scale(ptr, len) -> void {
    symbol scale;
    lang python;
  }
}
```

Top-level `import` may omit `lang` (defaults to `c`; printer omits `lang` when `C`). Class methods always print `lang`. `.sd` idents: `c`, `python`, `llvm`, `haskell`, `lean`. Unknown ident is `Err`. JSON field names: `classes`, `layout`, `methods`, `lang`.

G1–G4 / G5: not in this wave.

### Out of scope

`Library::call` / `call_python` as the class demo, inheritance, vtables in the region, default backing (C1c), class overlay field, `overlay arena` / `Type::Arena`, private fields, static/abstract/`super`, StarDust-written bodies, coupling tests to new trampolines.

### Zero-regression

Userspace named sets in Benchmark (1). Existing import JSON without `classes` still equals IR with empty `classes` and `lang: C`. Do not drop or swap cases.

---

## Done — Host class API

## Wave O3 — Host class API, multi-language methods

Tests: [`tests/import.rs`](tests/import.rs) (`class_c_method`); [`tests/python.rs`](tests/python.rs) (`class_multi_lang`, feature `python`). Host: [`src/class.rs`](src/class.rs) (`BoundClass`, `MethodLoader`).

**Status:** `done`

North-star demo: **one class**, **two languages**, **one instance’s bytes**, no copy between calls.

### Input

- **O1** done (method-shaped signatures can run). **O2** done (class IR exists).
- D1a, D3a: methods independently imported; several libraries + Python allowed on one class.
- F1a, F2a: no dtor; identity is `(region, offset)`.
- E2a: receiver is `(Ptr, Len)` to the instance span.

### Output

- Host API: `BoundClass::bind` / `call` given a class name and instance `(region, offset)`. Per-method host loader map (not in IR): `MethodLoader::Library` or `Python { script }`. `Library::bind_import` binds class methods by symbol (not `Ir.imports`). Host fills `Ptr { offset }` and `Len(layout.inline_size())`. Does not copy/re-encode the instance span (`as_ptr()` identity).
- Missing loader → `MissingLoader`; unknown class → `UnknownClass`; unknown method → `UnknownMethod`; `Lang` vs loader kind mismatch → `TypeMismatch`.
- Instance place is ordinary: caller picks backing and offset. Overlay override and bump allocate are parked.
- Demo: C `write_a` then Python `mutate` on the same span (I2/I3 callees). Lifetime = region. No dtor.

### Benchmark

1. **Zero-regression:** userspace named sets through O2. I3 `shared_region` still passes under `--features python`. Do not drop or swap cases.
2. **Named O3 set:** `class_c_method` (default `cargo test`): one C method; span `0xA1`; `as_ptr()` identity; prefix/suffix unchanged; unknown class / missing loader / unknown method are `Err`. `class_multi_lang` (feature `python`): C then Python; span `0xA2`; `as_ptr()` identity; prefix/suffix unchanged. Default `cargo test` does not require libpython.
3. Bind/call failure is `Err`, never silent `Ok` plus a copy trampoline.

### Review gate

Answered:

1. D1a + D3a: mix loaders; several libraries (per-method host map).
2. F1a + F2a: region lifetime; `(region, offset)` identity.
3. C2a: backing at place time. Overlay override parked.
4. **E2a:** `self` is `(Ptr, Len)`.
5. **Allocate:** ordinary only. No bump flag on the class API.
6. G1–G2 / G5: not this wave.

### Out of scope

Inheritance (**O4**), in-region vtable, ctors/dtors, F3b, multi-region calls, MSVC, StarDust method bodies, private/static/abstract/`super`, overlay override, bump class allocate, G1–G5.

### Zero-regression

Userspace named sets through O2. I3 `call_python` contract unchanged except invoking a class method (still memoryview, no `PyObject` in IR). Do not drop or swap cases.

---

## Done — Prefix inheritance

## Wave O4 — Single prefix inheritance

Tests: [`tests/layout.rs`](tests/layout.rs) (`prefix_ir`); [`tests/import.rs`](tests/import.rs) (`prefix_override`).

**Status:** `done`

### Input

- **O2** and **O3** done. OOP lock B1.
- Derived layout **starts with** base fields. Override = rebind a method name to another import. Host dispatch from the declared class of the view.

### Output

- Dumpable IR / `.sd`: optional `base Name;` after `layout Name;`. JSON `Class.base: Option<String>` (`#[serde(default)]`). Derived names a **complete** `Type::Struct` layout (prefix included). `methods` are this class’s own; inherited names merge at `BoundClass::bind` (root first, derived wins). Printer does not flatten inherited methods. Unknown base / cycle / non-prefix struct → `Err`.
- Host: a Derived view’s method is Derived’s import if overridden, else Base’s. A Base view over the same offset still calls **Base**’s imports (no tag/vptr). E2a `Len` is the declared class layout size. Builder `insert_class` requires the base class to already exist; `.sd`/JSON may list derived before base and validate at the end.
- Demo: Base `paint` → C `write_a`; Derived override `paint` → C `mut_b`.

### Benchmark

1. **Zero-regression:** userspace named sets through O3 `class_c_method`. `--features python`: `shared_region`, `class_multi_lang`. Do not drop or swap cases.
2. **Named O4 set:** `prefix_ir` (JSON / `.sd` / prefix retrieve; unknown base, cycle, non-prefix → `Err`). `prefix_override`: Derived `paint` is `mut_b` on the derived size; Base `paint` on the same offset is `write_a` on the base size (does not pick up the override); `as_ptr()` identity; prefix/suffix outside the instance unchanged.

### Review gate

Answered:

1. B1: single prefix, host dispatch, no runtime polymorphism, no multiple inheritance.
2. **G3 / G4:** later. No abstract/must-override. No `super`.
3. **`.sd`:** optional `base Name;`. Complete derived layout; own methods only in the map; merge at bind.
4. G1–G2 / G5: not this wave.

### Out of scope

Multiple inheritance, stored type tag / vptr, C++ virtual from unaware C, Python MRO/`__init__`/GC, StarDust-written bodies, F3b free-list, abstract, `super`, G1–G2 / G5.

### Zero-regression

Named sets through **O3**. Prefix retrieve must not break L1 path get/set on `repr_c` structs. Do not drop or swap cases.

---

## Wave O5 — Bump vs ordinary class allocate

Tests: [`tests/import.rs`](tests/import.rs) (`class_bump_place`). Host: [`src/class.rs`](src/class.rs) (`BoundClass::place`).

**Status:** `done`

### Input

- **O3** / **O4** done. L4 `Bump`. **R6** done (book order) but **O5** does not use `SlotAlloc`.

### Output

- `BoundClass::place(&mut Bump) -> Result<u64>`: `Bump::alloc` of the bound class’s layout `Type` (stored on `BoundClass` at `bind`). Ordinary path unchanged (`Region` + caller offset). No dumpable bump/backing field. **C1c** parked. Overlay override still parked.
- `call` still takes `(region, offset)` the caller already has (from `place` or ordinary). Drop `Bump` before `call` when `Bump` holds `&mut Region`.

### Benchmark

1. **Zero-regression:** R6 set plus `slot_reuse`. O3 `class_c_method` (ordinary) and O4 `prefix_override` still pass.
2. **Named O5:** `class_bump_place` ([`tests/import.rs`](tests/import.rs)) — two instances `place`d in one region (offsets 8 and 24 for 16-byte `Blob`); C `write_a` on each; spans do not overlap; prefix/gap/suffix unchanged; `as_ptr()` identity; ordinary `class_c_method` still the one-object path.

### Review gate

Answered:

1. `BoundClass::place(&mut Bump)`.
2. No dumpable bump/backing field.
3. Ordinary place unchanged (caller picks backing and offset).

### Out of scope

**C1c**, overlay override, `SlotAlloc` place, Python-only demo, default bump in IR.

### Zero-regression

Named sets in Benchmark (1). Do not drop or swap cases.

---

## Wave O6 — BoundClass extra call args

Tests: [`tests/import.rs`](tests/import.rs) (`class_extra_scalar`, `class_out_ptr`, `class_reject_foreign_ptr`). Host: [`src/class.rs`](src/class.rs) (`BoundClass::call_with`).

**Status:** `done`

### Input

- **O1–O5** done. [`BoundClass::call`](src/class.rs) always invoked the import with `[Ptr { region: 0, offset }, Len(inline_size)]`. Class methods were `(Ptr, Len) → Void` (O3 `class_c_method`). [`ImportFn::call`](src/loader.rs) already accepted extra [`CallArg`](src/loader.rs)s. **E2a:** one region for `BoundClass`.

### Output

- Keep `BoundClass::call(method, region, offset)` as today (sugar for empty extras).
- Add `BoundClass::call_with(method, region, offset, extra: &[CallArg])` that sends instance `(Ptr, Len)` **then** `extra`.
- `extra` may be `CallArg::Scalar` and `CallArg::Ptr { region: 0, offset }` only. Any other `Ptr.region`, or `Len` in `extra`, is an error. No new ABI; dumpable `Param::Scalar` / `Param::Ptr` already exist. No `Param::Struct`. `OwnedPtr` stays `IncompatibleImport`.
- Method `Import`s declare the full param list including extras. Bind rejects extras that are not `Scalar` or `Ptr` after the instance `(Ptr, Len)` pair.

### Benchmark

1. **Zero-regression:** O3 `class_c_method`, O4 `prefix_override`, O5 `class_bump_place` still pass. Matched sets for L/I/R/O1–O5 unchanged.
2. **Named O6** ([`tests/import.rs`](tests/import.rs); never name a test target `index`):
   - `class_extra_scalar` — extra `U64` in, `Ret::Scalar` out
   - `class_out_ptr` — extra `Ptr` into the same region; callee writes bytes there
   - `class_reject_foreign_ptr` — `Ptr.region != 0` fails before the foreign call

### Review gate

Answered:

1. `BoundClass::call_with`; existing `call` unchanged.
2. Extra args: `Scalar` and same-region `Ptr` only (**E2a**).
3. `Len` in extras is rejected (use `Scalar` for an out-buffer capacity).

### Out of scope

Python method extras, multi-region class calls, `Param::Struct`, filesystem product, UI toolkit.

### Zero-regression

Named sets in Benchmark (1). Do not drop or swap cases.

