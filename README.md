# StarDust

One git repo, two Cargo workspaces:

- **[stardust/](stardust/)** — StarDust Library (dumpable IR, named regions, imports, kernel UAPI).
- **[stardust-ui/](stardust-ui/)** — StarDust UI (cosmos, WebView2, Explorer cover). The crate is `stardust-ui`; the main exe stays `fs-viz`.

There is no Cargo **workspace** at this git root. A small `fs-viz` trampoline package is here so Tools → Rebuild on an older host (`cargo build -p fs-viz` from this folder) still builds [stardust-ui/](stardust-ui/). Library and UI work still run from those folders.

## Library

```
cd stardust
cargo test
```

Optional features: `python`, `llvm`, `haskell`, `kernel`. Contract: [stardust/waves.md](stardust/waves.md). Intent: [stardust/outline.md](stardust/outline.md). Syntax and layers: [stardust/README.md](stardust/README.md).

## UI

```
cd stardust-ui
cargo run --bin fs-viz
```

serves http://127.0.0.1:6006 (host file tree of this git root). Rebuild is `cargo build -p stardust-ui`. Contract: [stardust-ui/StarDust UI.md](stardust-ui/StarDust%20UI.md). Drive: [stardust-ui/desktop-drive.md](stardust-ui/desktop-drive.md).
