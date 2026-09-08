# StarDust

A **Rust-hosted library** for data layout and importing existing foreign functions. It is **not** a general-purpose language.

Two thin layers:

1. **To the OS** — how bytes are stored, retrieved, and represented (typed views on the same named region, no copy).
2. **To others** — call C, Python, LLVM, Haskell, and Lean on those bytes without translating the callee into Rust and without pre/post-processing when the payload is a StarDust region.

## Build and test

```
cargo test
```

Default `cargo test` is gcc/clang userspace only. Optional features: `python`, `llvm`, `haskell`, `lean`, `kernel` (Linux `/dev/stardust` or Windows `\\.\stardust` when present; otherwise tests skip).

## Where to read

- [outline.md](outline.md) — product intent (not executable)
- [waves.md](waves.md) — live contract (locks, parked gaps, named tests)
- [waves-done.md](waves-done.md) — archive of done L/I/R/O1–O6 waves
- [kmod/stardust-kmdf/README.md](kmod/stardust-kmdf/README.md) — Windows driver install
- [examples/cns_graph](examples/cns_graph) — CNS graph + Luna Light (Python BoundClass; `cargo test --features python --test cns`)

`cargo run -p fs-viz` serves the cosmos UI at http://127.0.0.1:6006 (host file tree of this repo, not GitHub’s repo page). Optional Cursor agent: set `CURSOR_API_KEY` (env or gitignored `.env`); Node.js is required for the sidecar. UI contract: [StarDust UI.md](StarDust%20UI.md). On Windows, View **StarDust UI** can replace Explorer folder windows (HKCU + `stardust-open` / `stardust-guard`; failsafe Ctrl+Shift+F12).
