# StarDust development waves

This file is the **live contract** for StarDust (locks, parked gaps, named tests). Do not implement from [outline.md](outline.md) or chat history.

There is **no remaining coding wave**; future named-wave I/O/B goes to [waves-done.md](waves-done.md). Unanswered picks: [choices.md](choices.md). Keep matched sets passing (matched sets, not just counts). Do not create [specification.md](specification.md) unless Developer asks.

**Authority:** Dumpable IR is the source of truth for layouts and imports. A later layout DSL is a copy. Do not “fix” the IR to match a syntax convenience.

---

## Product restatement

StarDust is a **Rust-hosted library** with a **language-neutral declaration surface**. It is two thin layers:

1. **To the OS:** explicit data layout — how bytes are **stored**, **retrieved**, and **represented**.
2. **To others:** import functions written in other languages **without translating them into Rust**, and pass data **without pre/post-processing** when the payload is a StarDust region.

Users do not have to write Rust forever; L1 may be a Rust builder because the **IR** is the declaration. Other languages meet StarDust through that IR and the **named byte region**, not by becoming Rust types.

---

## Locks (do not re-litigate)

### Identity

Hybrid: language-neutral declarations plus a Rust host/runtime. Declarations describe only storage, retrieval, representation, and imported signatures — not a general-purpose language. The host is the only thing that allocates, maps memory, and loads foreign code.

### Layout

Layered vision. **First layer:** C-like POD plus variable-length (slices, strings, length-prefixed buffers) on **named byte regions** (explicit size, alignment, placement).

Later layers are **views or overlays on the same regions**, not new worlds: columnar (L3), graphs/arenas (L4), bit-pack (L5), bitstream (L6) — all archived.

### Three verbs

- **Stored** — bytes in a region (kernel-visible backing).
- **Retrieved** — named field paths / offsets into that region.
- **Represented** — a **typed view** over the **same bytes** (no copy). If two views disagree, that is an error, not a conversion.

Debug/text dumps of IR or region bytes are **host tools**, not representation. Language-native objects (Python object, Haskell value) are an explicit later **export**, not representation.

### IR and syntax

Rust builder plus **dumpable IR** is authority. A small layout DSL in L2 only serializes that IR. Do not parse C headers as the declaration language. Do not make JSON/TOML the long-term language (a dump format may still be JSON if L1’s gate picks it).

### Import

Layered loaders. **First loader:** existing C binary/ABI (`.dll` / `.so`). Call **existing** functions; do not require C to compile against StarDust headers.

Zero-copy only if the signature is **region-compatible**: every argument and return is a scalar, a pointer into a StarDust region, a length, or a declared POD struct of those. Never silent copy. Non-compatible signatures **fail import** in the first layer (explicit copy/adopt is parked **H9**, not implied).

### Addressing

The region stores **offsets** (and lengths) from the region base. When calling C, pass **real pointers** (`base + offset`) and lengths so a `process(uint8_t *p, size_t n)`-style function is unchanged.

Interior C structs that embed raw process pointers are **not** region-compatible unless those pointers are into the region **and** we are not rewriting them in layer 1 — treat as incompatible.

### Ownership

StarDust or the caller allocates the region. C **borrows** for the call: may read/write in place; must not free or realloc those pointers. Wrapping a caller span is allowed if it meets the layout (size, alignment). Adopting C `malloc`’d returns is **out of layer 1**.

First C waves: **exclusive single-thread borrow** of a region for the duration of the call. Shared/concurrent access is later (not in the done book).

### OS and kernel

- L1: **heap** regions on **Windows and Linux**.
- R1: **mmap** and **file-backed** as the same region API.
- R2: userspace page control (`mlock`/pin, `madvise`, huge pages, NUMA) as explicit region options — syscalls, not a driver.
- R3: **Linux kernel module** whose only job is **region backing** (allocate / pin / map named regions into processes). Userspace stays declarations + loaders + views. R3 is **one process**.
- R4: **Windows KMDF** behind the same `Region::map_module` API as R3.
- R5: **kernel attach-by-name** on Linux R3 and R4 KMDF.

### First-layer demos

- L1: layout only (IR + region + retrieve + view).
- I1: one existing C call.
- I2: two C functions, one region, zero copy (first-layer north star).

### Book scope

All L/I/R/O waves are archived in [waves-done.md](waves-done.md) (Layout **L1–L6**, Import **I1–I7**, Region **R1–R6**, **O1–O6**). Open that file only when changing a done layer or chasing a named L/I/R/O test.

C, LLVM, Haskell, and Lean import as a platform `.dll`/`.so` through I1. Python (I3) is the exception: in-process CPython and a writable memoryview. Haskell and Lean add no language-native types to dumpable IR.

**Remaining:** none. Parked gaps below. Unanswered extras: [choices.md](choices.md) (G1–G2, G5; G3/G4 later).

### OOP

From [choices.md](choices.md). Do not re-litigate.

- **Class (A1):** A class is a named layout plus an attached method table in dumpable IR and `.sd`. Object bytes stay data-only (no vtable / function pointers in the region). The host dispatches method names to existing loaders. Method bodies stay foreign. Still not a general-purpose language.
- **Inheritance (B1):** Single inheritance. Derived layout is a **prefix** of the base (base field paths valid at offset 0 of the derived instance). Method table copies; **override** = rebind the same name to another import (including another language). Host dispatch from the **declared** class of the view. No stored type tag / vptr; a `Base` view over `Derived` bytes still calls Base’s imports. No multiple inheritance in this series. No C++/Python runtime (MRO, `__init__`, GC, unaware-C virtual calls).
- **Storage (C1b, C2a):** Class names a layout; that layout’s `Type` is the default kind (`Struct` → `repr_c`, `Columnar` → columnar, `Bitpack` → bitpack). Class does **not** dump an overlay field. Arena is **not** an overlay (`link` + host `Bump` remain L4). **O5** (done): `BoundClass::place(&mut Bump)`. Ordinary place remains (caller picks backing and offset). Overlay override still parked. Class does **not** declare default backing (**C1c parked**). Function-in-bytes is not a storage policy (A1).
- **Multi-language (D1a, D2b, D3a):** Each method is an independent import. One class may mix C, LLVM, Haskell, Lean, and Python. One class may span several `.dll`/`.so` files and a Python source. Dumpable IR records a **language/loader tag** on the method/import. That is metadata, not a language-native type. “No language types in IR” does not forbid a language label.
- **Call engine (E1b):** **O1** is done: `libffi` binds any mix of existing IR `Param` (`Ptr` | `Len` | `Scalar`) and `Ret` (`Void` | `Scalar`); `OwnedPtr` stays `IncompatibleImport` before dlsym; `(Ptr, Len) → Void` unchanged. No `Param::Struct`. **I7** is done: multi-region for `ImportFn` only (`call` + `call_regions`; `CallArg::Ptr { region, offset }`; dumpable `Param::Ptr` has no region names). `BoundClass` still one region / **E2a**. Class **calls** are **O3** (`BoundClass`). Receiver / `self` is **E2a**: `(Ptr, Len)` to the instance span. **O6** (done): `BoundClass::call_with` appends extra `Scalar` and same-region `Ptr` args; `call` is empty extras. `Len` in extras is rejected.
- **Lifetime / identity (F1a, F2a):** Object lifetime = the region, until `SlotAlloc::free` of that offset, or until the region dies (`Bump` slots have no individual free). No destructor in this series. Instance identity = `(region, offset)`; a freed offset may be reused. Use-after-free is not detected. **R6** (done) is host `SlotAlloc` (not IR). Typed handles later.
- **First-series extras:** Private fields, static methods, and StarDust-written method bodies are **not locked** (G1–G2 blank; G5 ask later). **G3** abstract / **G4** `super` are **later** (not **O4**). Out of scope until those gates are answered.

---

## Region-compatible C (layer 1 rule)

A C function is region-compatible when StarDust can call it by passing only:

- integer/float scalars by value, and/or
- pointers that are `base + offset` into a live region, and/or
- lengths/sizes that describe those spans, and/or
- POD structs whose fields are the above (no embedded foreign pointers, no `malloc` ownership transfer).

Outputs that must be zero-copy are **pre-allocated regions** the callee writes into. A `void *` return that the callee allocated is not region-compatible.

---

## Named tests

Matched sets for done layers (do not drop or swap). Full I/O/B: [waves-done.md](waves-done.md). Never name a test target `index`.

Default `cargo test` ([`tests/layout.rs`](tests/layout.rs), [`tests/import.rs`](tests/import.rs), [`tests/region.rs`](tests/region.rs)):

- **L1:** `round_trip_ir`, `store_retrieve`, `view_identity`, `alignment`
- **L2:** `ir_equality`, `builder_eq_file`
- **L3:** `columnar_ir`, `soa_view`
- **L4:** `offset_cycle`, `arena_c_walk`
- **L5:** `bitpack_ir`, `bitpack_view`
- **L6:** `bitstream_ir`, `bitstream_view`
- **I1:** `in_place_c`, `reject_incompatible`
- **I2:** `a_then_b`, `same_backing`
- **I7:** `two_region_c`
- **R1:** `mmap_c`, `file_round_trip`
- **R2:** `pin`, `advice`, `huge`, `numa`, `pin_mmap_c`
- **R6:** `slot_reuse`
- **O1:** `scalar_arg`, `scalar_ret`
- **O2:** `class_ir`
- **O3:** `class_c_method`
- **O4:** `prefix_ir`, `prefix_override`
- **O5:** `class_bump_place`
- **O6:** `class_extra_scalar`, `class_out_ptr`, `class_reject_foreign_ptr`

Feature-gated (skip-or-fail rules unchanged; [waves-done.md](waves-done.md)):

- **I3:** `shared_region` — [`tests/python.rs`](tests/python.rs), feature `python`
- **O3:** `class_multi_lang` — [`tests/python.rs`](tests/python.rs), feature `python`
- **I3 app:** `cns_smoke`, `luna_check_store`, `luna_smoke`, `luna_dict_yaml` — [`tests/cns.rs`](tests/cns.rs), feature `python` (CNS/Luna Light; [`examples/cns_graph`](examples/cns_graph))
- **I4:** `llvm_on_region` — [`tests/llvm.rs`](tests/llvm.rs), feature `llvm`
- **I5:** `haskell_on_region` — [`tests/haskell.rs`](tests/haskell.rs), feature `haskell`
- **I6:** `lean_on_region` — [`tests/lean.rs`](tests/lean.rs), feature `lean`
- **R3:** `module_backed` — [`tests/linux_module.rs`](tests/linux_module.rs), feature `kernel`
- **R4:** `kmdf_backed` — [`tests/kmdf.rs`](tests/kmdf.rs), feature `kernel`
- **R5:** `attach_two_process` — [`tests/attach.rs`](tests/attach.rs), feature `kernel`

CI: [`.github/workflows/ci.yml`](.github/workflows/ci.yml).

---

## Gaps parked (not waves)

Do not turn these into silent features inside numbered waves:

- Concurrency beyond exclusive single-thread borrow (H5)
- Security model for loading untrusted binaries (H6)
- IR versioning (H7)
- Silent copy or adopt-`malloc` compatibility shims (H9)
- MSVC shared-lib fixtures (H4; still [problems.md](problems.md))
- Class default **backing** in IR (C1c)
- Class overlay override at allocate (C2a extra; O3 uses the named layout only)
- In-region vtable / function pointers (A2/A3 not picked)
- Runtime `Base*` → `Derived` dispatch / stored type tag (B2 not picked)

If a wave cannot proceed without one of these, **stop and ask** — do not extend the wave.
