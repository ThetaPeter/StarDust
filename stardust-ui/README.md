# StarDust UI

Cosmos file tree, Inspect, and Windows Explorer/Desktop cover. Package name `stardust-ui`; the main binary is still `fs-viz` (HKCU / guard / view unchanged). Helpers: `stardust-open`, `stardust-guard`, `stardust-view`.

Depends on the library at [`../stardust`](../stardust) (path dep).

```
cargo run --bin fs-viz
```

http://127.0.0.1:6006 — the host file tree is the **git root** (both `stardust/` and `stardust-ui/` show as stars). Optional Cursor agent: `CURSOR_API_KEY` in gitignored `.env` here; Node.js for the sidecar (`agent/server.mjs` on 127.0.0.1:6007).

Rebuild from this folder:

```
cargo build -p stardust-ui
```

Optional `--features python` (Inspect/CNS), `llvm`, `haskell`.

Contract: [StarDust UI.md](StarDust%20UI.md). Drive tools: [desktop-drive.md](desktop-drive.md). Do not write `%APPDATA%\StarDust` unless the Developer says yes.
