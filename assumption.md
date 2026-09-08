# Assumptions

## Confirmed

- **Hybrid:** StarDust is language-neutral declarations plus a Rust host. Detail: [waves.md](waves.md) Locks.
- **IR authority:** Dumpable IR is the source of truth; `.sd` is a thin syntax copy. Detail: [waves.md](waves.md) Locks.
- **Book:** [waves.md](waves.md) is the live contract; [waves-done.md](waves-done.md) archives L/I/R/O.
- **L6:** `Type::Bitpack` is span-only with the same packer. Detail: [waves-done.md](waves-done.md) L6.
- **I7:** Host `CallArg::Ptr { region, offset }` on `ImportFn` only. Detail: [waves-done.md](waves-done.md) I7.
- **F3b / R6:** Host `SlotAlloc` (`alloc` / `free`); not IR. Detail: [waves-done.md](waves-done.md) R6.
- **O1:** `libffi` trampoline over existing IR; Windows GNU uses [`vendor/libffi-sys`](vendor/libffi-sys). Detail: [waves-done.md](waves-done.md) O1.
- **O2:** Class is a layout name plus method table. Detail: [waves-done.md](waves-done.md) O2.
- **O3:** `BoundClass` plus per-method `MethodLoader` (not in IR). Detail: [waves-done.md](waves-done.md) O3.
- **O4:** A class may name one `base`; layouts must be a struct prefix. Detail: [waves-done.md](waves-done.md) O4.
- **O5:** `BoundClass::place(&mut Bump)`; **C1c** parked. Detail: [waves-done.md](waves-done.md) O5.
- **O6** `(Latest)`: `BoundClass::call_with` extra `Scalar` / same-region `Ptr`. Detail: [waves-done.md](waves-done.md) O6.
- **Fixture names:** crates `stardust-c` / `stardust-ll` / `stardust-hs` / `stardust-lean`; attach env `STARDUST_W16_*`; region temp `stardust-w4-*.bin`.
- **OOP:** First-series extras remain unanswered. Detail: [waves.md](waves.md) OOP; [choices.md](choices.md).

## Unconfirmed

- **G1–G5:** See [choices.md](choices.md). Out of scope until answered. (Not archive G1–G7.)
