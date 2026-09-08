# StarDust UI

Daily-dev shell at `http://127.0.0.1:6006` (`cargo run -p fs-viz`). The cosmos is the **host file tree** (this StarDust repo by default). Dumpable IR is the source of truth for Inspect; `.sd` is a copy. This file is the UI contract. It is not a language wave.

## Shell

- **Top:** File (Open folder… — replaces the **focused** pane’s root; **Desktop**; **This PC**) / Edit / View (one **2D/3D** toggle; New split; Refresh; Tile/Cascade; sort; Galaxy; one **StarDust UI / Windows UI** toggle) / Tools (`cargo test`; **Open in Cursor**; **Refresh web**; **Rebuild StarDust UI**) / Help (Keyboard shortcuts, About) | Search (file/dir names).
- **Body:** cosmos view(s) + floating file windows | collapsible Agent (right). Collapsed: a left-pointing triangle tab (not the word Agent).
- **Per cosmos view:** minimize (–), close (×) next to it, **star pin** (☆ / ★ toggle; no duplicates), new split.
- **Bottom pin area:** permanent pins + **minimized** items only (not already-visible windows). Same path + kind collapse to **one chip** (first-seen order). Label is the title, plus ` (n)` when **n** minimized copies in that group is > 1 (`Name`, never `Name (1)`). ★ if the group has a permanent pin, else ☆. Click restores the latest minimized copy (count drops); with no temps, click opens the permanent. Ctrl+click unpins the permanent and leaves minimized copies. Drag chips to change group order; that order persists.

Look: living two-layer sky (shell ambient field plus per-pane far dust that tracks that pane’s camera). Directories are soft Hubble-like nebulae or faint spirals; files are photographic cores with diffraction spikes and bloom (not graphic icons). Object glow is a bit brighter than the first Hubble pass; the void stays dark (not milky). Chrome is recessed spacecraft wells. Type is Bahnschrift / Segoe UI Variable, 12px chrome and ~9–11px star labels. Hubble-style `cosmos (1)–(5).png` stills in the repo root are look-dev only and are not loaded as UI backgrounds.

## Cosmos

Each view shows **direct children** of one directory as stars. `target/` at the StarDust root is one star; entering it still shows only that folder’s direct children. The **Desktop** view is an Explorer-like merge of the user Desktop known folder and Public/Common Desktop (see below).

- Directories pick one of ten looks from a stable name hash: emission nebula, reflection nebula, planetary nebula, supernova remnant, spiral, barred spiral, elliptical, irregular, globular cluster, edge-on disk. Files pick a spectral color (O/B/A/F/G/K/M/Wolf–Rayet) and a morphology (4-spike, 6-spike, halo, binary, Airy rings). Size still scales with zoom. Hover and select brighten the core and widen the bloom — there is no stroked ring.
- Background: a shell-wide **baked** deep field (nebulae, faint galaxies, spiked stars) with two slow parallax layers and a live twinkle overlay. Each cosmos pane also has far dust and faint nebula patches, seeded from the current folder, drawn as a **screen-space wrap** of that field so pan/orbit still parallax but **zoom-out never shows a decoration rectangle vs black void** (the tile covers the pane at every 2D scale, including CNS 36 / folder 6, and 3D orbit zoom-out). File/directory stars stay in world space. Far decorations stay **smaller** than file and directory glyphs. Pane fill stays mostly transparent so the shell sky shows through.
- Twinkle: files and dust pulse opacity and spike/halo size on staggered phases (~0.6–1.4s for files; slower and weaker for dust). Directories breathe (slow core pulse) and do not sparkle like files.
- Names are visible (stronger when zoomed in). Hover highlights. **Hold Ctrl** to emphasize nearby names (Ctrl is also the navigate modifier for click and scroll).
- Breadcrumb of the current path on the view: click an ancestor to scene-swap there (overflow menu if the path is long). Crumbs are the only visible address (no second full-path chip). Ctrl+L types a path (including drive roots); Enter goes there.
- Galaxy/force layout runs **once** per directory then freezes. Drag a star to move **only that star** (other selected stars stay put). In 3D, drag follows the cursor including height; Shift+drag moves height only. Positions persist per directory **and** separately for 2D vs 3D. Sort is a temporary arranged mode; **Galaxy** restores the last galaxy/manual positions. Right-click **Image** submenu (click or hover; stays open to pick): **Set image…** (PNG/JPEG, WebView2 file dialog), **Reset image**, and for folders the built-in looks — looks persist under `--desktop`.
- New file/folder stars appear near the cursor (or view center). Existing stars stay put.

Process start root is this StarDust repo. Each split may **Open folder…** to its own root. **StarDust UI** mode uses the real Desktop known-folder as the session root (including OneDrive Desktop when that is the Desktop).

## Windows Explorer / Desktop

StarDust can own **filesystem folder windows** and the **Desktop icon surface**. Taskbar, Start, Alt+Tab, UAC, Settings, and Open/Save dialogs stay Windows. This is not a Winlogon shell and not a kiosk.

- **StarDust UI / Windows UI** (one View-menu button that shows the current mode and switches on press, plus the `stardust-guard` tray): StarDust UI hides Explorer desktop icons and, when `stardust-open.exe` sits next to `fs-viz.exe`, writes **HKCU** Directory/Drive `open` verbs to that trampoline (backup first). Windows UI restores that backup and shows Explorer folders/icons again. The View toggle uses the same `setUiMode` path — Explorer restored, cosmos kept (`float_stardust_window`). The button label flips immediately, toasts **Switching…**, then success or fail (12s client timeout rather than an infinite wait). Re-entry while a switch is in flight is ignored. Switching to StarDust UI does not replace a focused **Inspect in StarDust** pane with Desktop. The Sort submenu opens to the right so it cannot eat that click. Tray / **Ctrl+Shift+F12** stay the emergency failsafe. No extra keyboard chord for UI mode. Mode is `%APPDATA%/StarDust/ui-mode.json` (not the registry for the in-app toggle itself).
- **Open in Explorer** uses the `explore` verb so it does not recurse through the trampoline. Recycle Bin *view*, Control Panel, and other virtual folders stay Explorer. `.lnk` shortcuts open via the OS (same as Explorer).
- **Failsafe:** `Ctrl+Shift+F12` (guard process) restores Windows UI immediately. Guard also failovers if HTTP `GET /api/healthz`, the host heartbeat, UI pulse, HKCU verbs, or desktop-icon hide-state go red (one host restart on yellow, then stay on Windows until you pick StarDust UI again). `stardust-guard` may stay running in Windows UI (health + hotkey); it must not refresh Explorer (`SHChangeNotify` / desktop `ShowWindow`) when Windows UI is already restored.
- **Native window:** `fs-viz --open PATH` and `fs-viz --desktop` host the UI in **WebView2** (not Edge `--app=`). Browser accelerators stay off (Find, DevTools, Ctrl+scroll page zoom, swipe-back, Edge context menu). The page’s own keys and menus run instead. `%APPDATA%/StarDust/webview-profile` is still the profile folder. WebView2 Runtime ships with Edge; `WebView2Loader.dll` sits next to `fs-viz.exe`. The `stardust-view` window/taskbar/tray icon is the luna silhouette as a **derived** PE `RT_GROUP_ICON` (`crates/fs-viz/assets/stardust.ico`: knockout paper → transparent, black ink → white `#FFFFFF`; no cream/white plate). `luna-silhouette.png` stays black and is not recolored; the tab favicon is the derived `luna-icon.png` (same white-on-transparent art — CSS `filter` does not apply to `<link rel="icon">`). Win11 taskbar uses the PE icon, not only `WM_SETICON`. Rebuild `fs-viz` (Refresh web is not enough). Default `cargo run -p fs-viz` is still HTTP-only for daily-dev. Build the package so `stardust-open.exe`, `stardust-guard.exe`, and `stardust-view.exe` land next to `fs-viz.exe` (`cargo build -p fs-viz`).
- Desktop: StarDust is a **borderless work-area window** (no OS title bar, no inner cosmos glow) covering the wallpaper. The host window is `StarDustHost` (WebView2 child), not a clipped Edge caption. Translate prompts are suppressed (WebView2 flags + profile prefs + `translate="no"`). Win11 Spotlight paints above WorkerW children, so we do not embed into WorkerW. View → Windows UI restores Explorer’s desktop view and window chrome (the floated cosmos window keeps Close). The desktop cover does not show an OS Close tooltip. Leaving to another app (Cursor, Alt+Tab) and coming back **keeps the cover** and the live cosmos — **View → Refresh is not required** (Refresh remains a full reload). Every visible `StarDustHost` is restyled as the work-area cover while StarDust UI is on; WebView2 is shown/resized/refocused on activate, and an in-flight Inspect 字 fetch retries without re-opening the session.
- **Desktop stars:** the session Desktop known folder is listed as Explorer’s merged namespace: user Desktop plus `%PUBLIC%\Desktop` (Common Desktop). User-folder names win on collision. Public-only items keep an absolute `path` so open / `.lnk` use that file (not `userDesktop + name`). Files are not copied or symlinked onto the user Desktop. Recycle Bin, Control Panel, and other virtual folders stay Explorer.
- **Login:** while StarDust UI is on, HKCU Run `StarDustUI` starts `fs-viz --desktop` at sign-in (taskbar/Start stay Windows; this is not Winlogon `Shell`). Windows UI deletes that Run value. `StarDustGuard` stays in Run for the failsafe and, while mode is Stardust, reinstalls `StarDustUI` if it is missing (path = current `fs-viz.exe`). At sign-in the cover waits until `GET /api/healthz` on `127.0.0.1:6006` returns 200 before WebView2 Navigate (boot grace 60s, then start `--desktop` and wait again); it retries Navigate on a timer so login does not end on Edge `ERR_CONNECTION_REFUSED` / 無法連線. Dual Run (`StarDustGuard` + `StarDustUI`) reuses an existing `StarDustHost` when HTTP is up — it does not WM_CLOSE the cover. A leftover `stardust-view` (taskbar) starts `fs-viz --desktop` if HTTP is down. The guard does not `taskkill` that `--desktop` host or failover to Windows while the host is still coming up (process-local boot grace, not stale `armed_at`). If mode is already Windows, it stays Windows until you pick StarDust (no UI Run key). Tray **StarDust UI** also starts `fs-viz --desktop`.

## Gestures

| Gesture | Action |
| --- | --- |
| **Click, then hold the second click and move** | Pan the camera (2D and 3D) when the second down is **empty**. On a star, the drag moves that star instead. A released double-left-click (no move) on a star opens it. |
| **Ctrl+scroll** | Camera zoom / dolly in the current scene (does not enter or leave). Always zooms; never leaves. |
| **Scroll-in** on a **directory** star | Fly-into a directory only after accumulated scroll-in on that same star; debt `100 × (720 / max(96, short))` (smaller pane → more). Until then, zoom. First event of a gesture / first event on a new star does not enter. |
| **Scroll-out** | Leave (fly-out, then parent) only when this view already frames every star (pad max(24px, 12% of the short side); 0 stars count as framed) **or** zoom-out is maxed, **and** only after a further accumulated scroll-out whose debt is `140 × (720 / max(96, short))` (smaller split → more scroll-out). A gesture that **starts** with any star clipped (wheels within 350ms are one gesture) only zooms — it never leaves. After idle 350ms+ with the scene framed, extra notches still zoom until min zoom, then wait until that debt is met; the first framed event of a gesture never leaves. Works during an in-flight fly. |
| **Ctrl+click** a directory star (no drag) | Enter that directory. |
| **Ctrl+click** empty space (no drag) | Leave to the parent directory. |
| Middle-drag | 2D: pan. 3D: orbit (turntable). |
| Alt+left-drag empty | Pan. |
| Drag a star (no Ctrl) | Move **only that star**. 3D: follows the cursor in camera space, including **height (Z)**. **Shift+drag** in 3D changes height only. Does not start a marquee. |
| Left-drag empty | Marquee select (rubber-band). Replaces selection; **Ctrl** (or meta) or Shift unions. Empty marquee clears unless adding with no hits. Does not pan. |
| Left-click | Select (branch). Does not pan/orbit. Shift+click is a **spatial** range. Selected stars keep a bright halo and a label chip. |
| Double-left-click | Open (file → editor/OS; `.lnk` → OS; directory → enter). |
| Right-click | One menu. On a star: Open / Open in new split / Inspect IR / Pin, plus Send to Agent, Cut/Copy/Paste, Copy path, Rename, Delete, Properties, **Open in Explorer**, and an **Image** submenu. **New file/folder** only on empty space. Multi-select Open/Pin/Open in new split applies to the whole selection. |
| Enter | Open (file) / Enter (directory) on the focused/hovered star. |
| Alt+Up | Parent folder (This PC at a drive root). |
| Alt+Left / Alt+Right | Back / forward in that pane’s folder history. |
| F2 | Rename in place (Enter commits, Esc cancels; collisions rejected). |
| Delete | Confirm, then send to OS Recycle Bin / Trash. |

**Right-click menu**

- Directory: Open (enter this view), Open in new split, Pin / Unpin.
- File: Open (float), Inspect IR/layout (enabled only for `.sd` / known layouts), **Inspect in StarDust** (single `.sd`; `world.sd` CNS/Luna instance when the host is built `--features python`; other `.sd` greyed — no instance illustration yet), Pin / Unpin.
- Multi-select: **Open N** opens every selected file and, if more than one directory is selected, each extra directory in a new split (a single selected directory still enters this view). **Open N in new splits** opens every selected directory in its own split. **Pin N** / **Unpin N** toggles all selected stars.
- Empty space only: New file/folder, Paste.
- With a star (or selection): Send to Agent, Cut, Copy, Copy path, Rename (single only), Delete, Properties, Open in Explorer.
- **Image** (click or hover; stays open to pick, single star): Set image…, Reset image, and for folders the built-in looks.

## Keyboard

In-app chords. Cosmos/file chords do not run while typing in search, the path field, rename, or a file editor (**Ctrl+S** still saves). **Agent chords** (**Ctrl+I** toggle, **Ctrl+Shift+I** focus composer, new thread, stop, send selection), **Ctrl+F5** (Refresh web), and the overlay run from anywhere, including those fields and the composer — same idea as Cursor. Composer **Enter** sends; **Shift+Enter** inserts a newline. **Ctrl+click** / **Ctrl+scroll** stay navigate / zoom (not keyboard chords). The native window is WebView2 with browser accelerators off, so Find / DevTools / page-zoom do not steal those keys. Opening `http://127.0.0.1:6006` in a normal Edge tab still uses Edge’s keys. Not in v1: Win+E, F11, Space preview, Alt+underlined menus.

Discovery: **Help → Keyboard shortcuts**, **Ctrl+/**, or **F1** (F1 is `preventDefault` so Edge does not open its help). **Esc** closes the overlay; backdrop click also closes it.

### Navigation

| Key | Action |
| --- | --- |
| **Enter** | Open / enter the focused or hovered star |
| **Backspace** | Parent folder (same as Alt+Up; ignored in text fields) |
| **Alt+Up** | Parent folder (This PC at a drive root) |
| **Alt+Left** / **Alt+Right** | Back / forward in that pane’s folder history |
| **Ctrl+L** | Focus the focused view’s path field (selects contents when possible) |
| **Ctrl+F** | Focus search |
| **F5** | Reload the focused view’s children |
| **Ctrl+F5** | Refresh web (JS/CSS/HTML from disk; no Cargo). Runs while typing |
| **Ctrl+N** | New split of the focused pane |
| **Ctrl+W** | Close the focused cosmos pane (not file windows). Last pane cannot close. |
| **Ctrl+Shift+2** / **Ctrl+Shift+3** | Focused view 2D / 3D |
| **Esc** | Close menus, search dropdown, and the shortcut overlay |

### Files

| Key | Action |
| --- | --- |
| **Ctrl+A** | Select all stars in the focused cosmos view |
| **Ctrl+C** / **Ctrl+X** / **Ctrl+V** | Copy / cut / paste |
| **Ctrl+S** | Save the focused editor |
| **F2** | Rename the focused star (Agent title / tab / History list F2 is separate) |
| **Delete** | Confirm, then Recycle Bin / Trash |
| **Ctrl+O** | File → Open folder… |
| **Ctrl+D** | File → Desktop |
| **Ctrl+Shift+P** | File → This PC (**Ctrl+P** is left alone) |
| **Ctrl+Shift+N** | New folder in the current directory |
| **Alt+Enter** | Properties of the focused/selected star |

### Agent

| Key | Action |
| --- | --- |
| **Ctrl+I** | Toggle the Agent panel from anywhere (search, editor, composer). Opening focuses the composer |
| **Ctrl+Shift+I** | Focus the composer (open the panel if it is closed; do not collapse). Same global rule as Ctrl+I |
| **Ctrl+Shift+A** | Send selection to Agent (opens the panel, attaches stars, focuses the composer) |
| **Ctrl+Shift+T** | New agent thread |
| **Ctrl+Shift+.** | Stop the current agent run |
| **Enter** / **Shift+Enter** | Send / newline in the composer |
| **F2** | With a thread tab focused, or a thread selected in the History list: rename that chat. Cosmos F2 still renames files |

### Shell

| Key | Action |
| --- | --- |
| **Ctrl+/** / **F1** | Toggle the keyboard-shortcut overlay |
| **Ctrl+Shift+F12** | Failsafe: restore Windows UI (guard process; not reused in-app) |

## Windows and pins

- **Open file** (text): floating window (drag the title to move; drag any edge or the bottom-right grip to resize; **Save** on the title bar when dirty, also Ctrl+S; min/max/close). Dock into the focused cosmos view as tabs (Save is on the tab strip); undock to float. **Tile windows** / **Cascade windows** (View menu) arrange those file windows over the cosmos — they do not retile cosmos splits. If the files were docked as tabs, Tile/Cascade undock them first. Greyed out when no file window is open. Monaco editor; dirty close = Save / Don’t save / Cancel.
- **Non-text:** OS default application; no in-app editor.
- **New split:** ⊞ duplicates the current folder and camera into two panes along the longer edge of the focused pane (side-by-side if wider, stacked if taller); it does not open the process default root. Menu “Open in new split” still opens the chosen directory in the new pane. Further splits divide that pane the same way. Sash still resizes. File windows float over the focused view.
- **Minimize (–):** hide that cosmos pane to the pin bar (**one temp pin per view**, even if that folder is already ★) and **focus the latest still-open view**. If it was the only pane, a live view at the default root takes its place (no blocking overlay). File minimize is still one temp per window. Duplicate collapsed views of the same path show as one `Name (n)` chip. Restore from the pin bar.
- **Close (×):** remove that pane (no pin). The last live pane cannot be closed (use minimize).
- **Star pin:** ☆ / ★ toggles a permanent pin for the current folder (or a star from the right-click menu). The same path is stored only once. A ★ plus collapses of that folder is still one chip (`★ Name (2)` when two are minimized). Ctrl+click a ★ on the pin bar to unpin the permanent (minimized copies stay).

## Search

Name search over the tree (the current split root is included when its name matches). When the **focused pane** is **Inspect in StarDust** (CNS/Luna abstract space), type in the **Word header find box** (placeholder `Search Word / Enc / Post / Cns`; **Ctrl+F** / **Ctrl+L** focuses it) or the top-bar Search while that pane is focused. Matches **字**, Luna **Enc** / **Post**, or **Cns** (e.g. `1-5153`); choosing a hit **flies** to the already-placed 字 (does not enter the 字 neighborhood; Enc/Post/Cns do not spawn). Search still works after a neighborhood double-click: it leaves similar-words, flies on the catalog plane, then empties the box (same as file search). File-tree search: walks **all open split roots**, the **focused folder**, and also `%USERPROFILE%\Desktop`, `%USERPROFILE%\OneDrive\Desktop`, `%PUBLIC%\Desktop` (and `HOME/Desktop` off Windows) when those folders exist. Desktop extras use their own visit budget so a large repo walk cannot starve them. The dropdown shows **Searching…** immediately, then results (or **No matches**); it is `position: fixed` under the box. Results stay open until a hit is chosen, Escape, or the query is cleared; choosing a hit empties the box and the list. Rank: path jump (existing absolute path, or a child of the current search root / focused folder, hidden folders included), then the current folder, then exact / prefix name matches, directories before files. `target/`, `node_modules`, and `AppData` / `Application Data` are not listed as hits when unmatched, but a matching name or an explicit path is a jump target; search still does not recurse into those folders, so they cannot eat the visit budget. Matching sibling names in a folder are collected before descending; matching directories are descended first so Desktop is found before a deep walk can eat the visit budget. Hover a hit highlights its parent directory on the map. Enter in the search box opens a hit; cosmos Enter is ignored while the search field is focused.

## Agent

Cursor-like chat, StarDust styling, collapsible; last open/collapsed state is remembered. Threads switch from **tabs on the Agent header** (not a left rail). **+** starts a thread. A **History** clock (top-right) opens the thread-list popover; **Rules** is the next button to the right (same corner Cursor uses for history) and edits the global rules string in a popover. Composer mode: **Agent** / **Plan** / **Ask** (Ask is SDK `mode: "agent"` with edit/delete/applyAgentDiff disallowed — there is no SDK `"ask"`). Model dropdown, persisted per thread. Sidecar prepends a non-empty Rules box on every send; missing `agentRules` fills the AppData lock default; a cleared box stays empty. There is no assistant-name bar on the header — the **tab is the chat title**. Built-in Cursor tools (shell, read, grep, …) stay enabled (`customTools` is additive; do not pass `tools: []`). Overlay cwd is still the temp `stardust-ws-*` junction folder, not the Desktop.

- Double-click / **F2** a thread **tab** (or a row in History) to rename that chat. Tab and list titles use a text cursor and a rename tooltip. Thread titles are PATCHed to the sidecar when it is up. A new chat stays **New thread** until the first reply finishes; then the sidecar names it (2–6 words for the user’s goal, not a slice of the prompt or an `@` filename). Tab **×** (**Close chat**) closes the tab only: the thread stays in History (`open: false` in `agent-history.json`; sidecar id is kept so resume still works). History **×** (**Delete chat**) permanently removes the chat from tabs and History (saved immediately; sidecar DELETE when it is not local). Click a History row to reopen it as a tab. Closing an empty untitled **New thread** (no user/assistant text) drops it entirely so it does not leave a History ghost.
- Chat history lives in `agent-history.json` (header tabs and the History popover). Closed chats stay in History until deleted. The sidecar’s in-memory thread list is **not** imported on boot. After reload, saved messages are restored into the panel and the next send continues that thread (sidecar/SDK resume when possible; otherwise a new sidecar thread with the saved conversation prepended). Empty **New thread** tabs with no messages are dropped on load.
- Double-click a user message to edit it (assistant rows are not editable). Enter commits, Esc cancels, Shift+Enter inserts a newline. Clicking anywhere else (including empty chat, composer, tabs, or another Agent button) or a mode/model change cancels the edit without committing. If later turns exist, confirm **Discard later messages**. That truncates the thread after the edited prompt, disposes the sidecar agent, and sends the new text as a fresh run. The last user message with no later turns still recreates the sidecar agent (no confirm).
- Local TypeScript sidecar (`@cursor/sdk`). `CURSOR_API_KEY` from the environment or a gitignored local env file. Missing key: clear error, no crash. Replies stream into the panel; the workspace folder is reused until split roots change.
- Workspace is a **temporary folder** of junctions/links to every open split root, with paths mapped back in the UI when possible.
- **Send to Agent** is Cursor `@` attach of the **selected** star(s) (marquee or Shift range, not only the one under the cursor) on the **current** thread: right-click menu or **Edit → Send to Agent**. The agent panel opens with those paths as chips. On Send, every visible `@` chip is included with the prompt (file content is inlined when it is text and not huge; chips-only with an empty composer is a valid send). The send does **not** include a text snapshot of the cosmos; the agent must call tools to see the UI. While a run is in flight, the transcript shows **action** rows (Thinking…, then tool names) in the same bubble style in **stream/time order**, not parked above the reply; replies still stream. **Stop** cancels the in-flight run.
- **UI tools** (Cursor SDK custom tools, focused pane only): **look-at-view** (JPEG of the focused cosmos/folder canvas — not menus or the Agent panel), **navigate** (folder path or This PC), **open** (star names/paths), **select** (star names), **pin** (star names/paths), **copy** (duplicate on real host paths; optional dest directory, omit dest to duplicate beside the source; Explorer names `Name - Copy.ext` / `Name - Copy (n).ext`; never the temp `stardust-ws-*` overlay). The sidecar HTTP-calls the host; the page polls a short inbox and executes via `window.SD`. The agent cwd is a temp junction overlay of split roots, not the Desktop — “Copy this file” must use **copy**, not SDK Write.

## Persistence

User config files (not in the git repo): `ui-layout.json` (pins, splits, open files, agent open/width/model, star positions/images), `agent-history.json` (assistant name, agent rules, current thread, threads with messages / model / agentMode / open), and `ui-mode.json` (StarDust vs Windows UI). History is a sibling of layout under `%APPDATA%/StarDust/` (or the same config dir off Windows). Transcripts and rules are **not** stored in `ui-layout.json`. `--desktop` / `--open` do not replace views/split/pins in `ui-layout.json`; they still merge `starPos` / `starLooks`. `agent-history.json` still loads and saves. Default root on a new process is this StarDust repo until Open folder in a view, unless **StarDust UI** is on (then Desktop).

Live **watch** updates stars when the folder changes.

## Mutate and drag-and-drop

Rename, Delete (Recycle Bin), New, Copy, Cut, Paste change the real disk. Paste and New place stars near the cursor. Drag from OS Explorer into a view copies (Ctrl) or moves (Shift) when the host allows. Drag between cosmos splits copies or moves internally.

## Inspect and tests

- **Open** a `.sd` file: IR text in the floating editor (same as other text). That is how you read dumpable IR.
- **Inspect IR/layout:** floating pinnable window. Pretty type/field tree first; dumpable IR behind a toggle. Only `.sd` / known layouts; other files: disabled or “not a StarDust layout.”
- **Inspect in StarDust** (right-click one `.sd`): opens a **new cosmos split** (does not replace the folder you were in). Close **×** / **Ctrl+W** on that pane to leave. For [`examples/cns_graph/world.sd`](examples/cns_graph/world.sd) the host allocates `WorldLay`, `import_all` (Python BoundClass; `fs-viz` must be built `--features python`). The pane shows an **Inspect in StarDust — importing Word…** overlay with elapsed seconds until import/catalog + one layout of every word returns. Default plane is that **cloud of all catalog words**, laid out once: IDS → strip `(U+…)` and IDC (`⿰⿱⿲…`) → **leaves** (empty IDS uses the 字 itself); each leaf hashed to a `leaf_axis` direction; word home = average of those unit directions **× 1600** (`PLANE_SCALE`); plus a **golden spiral from catalog index `i`** (import order, not IDS): radius `22 × √(i+1)`, angle `i * GOLDEN` (`GOLDEN = 2.399963229728653`); jitter ~**±48** px from the character; then **16** force iterations (weak pull toward other words that share a hashed leaf, coefficient **0.018**, few neighbors, plus local repulsion so glyphs do not stack). Map distance is that hash + spiral + unstick — **not** “how similar the IDS strings are,” not a Jaccard embedding, not a polar disk, not an origin hub, and not incremental rim expand. Process-lifetime `layout_seed` (first new session stores mixed nanos; same until fs-viz quits) mixes into **leaf_axis and spiral phase only** — not re-rolled on Inspect. Closing the pane does not drop the worker Session; a second Inspect of the same `.sd` reuses the session id and `placed` (no second `import_all`, no re-layout). Frustum: `STAR_CAP` 700, in-box, hash subset when over cap (not 350 nearest-to-center, not 1-per-cell). First frame is a subset of **all** placed, then `frameCnsCloud`. 字 sprites stay small (folder Hubble look / `starR` max 34 is files only). Wheel zoom-in on Inspect 2D goes to scale ~36 (folder cosmos max stays 6); 3D dolly min is ~15 (folder stays 60). First load frames the whole cloud (not max zoom). Zoomed-in star lists are the view box. 字 name labels appear when hovered/selected/highlighted, when ≲35 字 are on screen, or when pixel gap is comfortable (`scale * 5 ≳ 36`); folder file labels are unchanged. Search flies to the already-placed 字 (does **not** enter neighborhood), including while similar-words is open: a successful 字/Enc/Post/Cns hit leaves neighborhood and flies on the catalog plane. Enc / Post / Cns stay search + inspector only (they do not spawn). Double-click enters that 字’s **neighborhood**: rank by shared leaves / Jaccard (at least one shared leaf, **cap 80**); that view is the IDS-nearness picture; display coords do not overwrite plane `placed`. Leave neighborhood via **Escape**, **Alt+Up**, or double-click empty cosmos (not a star); Inspect stays open (pane **×** still closes the view; **−** still splits). Scroll-out on Inspect zooms (does not leave). No focused-字 chip or stack crumb left of search. After Rebuild, a stale session id re-opens (`POST /api/cns/open`) and retries stars once instead of showing **CNS session missing**. The header has a find field (placeholder `Search Word / Enc / Post / Cns`); **Ctrl+F** focuses it. Select/hover shows Ids, SequenceLevel, `stroke_order`, `components_of`, Enc, Post via BoundClass query/result slices. Other `.sd` files: menu greyed (“no instance illustration”). Pane is ephemeral (not a filesystem path in `ui-layout.json`). `cargo run -p fs-viz --features python` then http://127.0.0.1:6006. Host layout: **Tools → Rebuild StarDust UI**. Session-missing retry JS: **Tools → Refresh web** (Rebuild also picks up JS).
- **Tools → cargo test** runs `cargo test` (default features) in the cosmos root (StarDust repo / focused project) in a pinnable window. The wait line ticks elapsed seconds until cargo returns. On Windows GNU, if `CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER` is unset, the host picks rustc’s self-contained `x86_64-w64-mingw32-gcc.exe` from one rustup toolchain directory (no recursive `.rustup` walk). The agent may also run tests.
- **Tools → Open in Cursor** opens the Cursor GUI on this StarDust repo folder (same `workspace_root` as Rebuild) via `ShellExecuteW` on `Cursor.exe` with `--classic --new-window "<folder>"` (flags before the folder; not `cursor.cmd`, and not `CREATE_NO_WINDOW` — that hid the window). `--classic` matches the classic editor (a tab appears only when the user clicks a file; newer Agents/Glass auto-opens edited files). `--new-window` so an already-running Cursor cannot swallow the click. Typical path `%LOCALAPPDATA%\Programs\cursor\Cursor.exe` (also `Program Files\cursor\Cursor.exe` and `Program Files (x86)\cursor\Cursor.exe`). Toast if Cursor is missing.
- **Tools → Refresh web** (Ctrl+F5) reloads the page from `crates/fs-viz/web/` on disk (layout flushed first); it does **not** run Cargo or relaunch `fs-viz.exe`. Use this after JS/CSS/HTML edits. Rebuild remains the native compile + `--desktop --replace` path.
- **Tools → Rebuild StarDust UI** runs `cargo build -p fs-viz` in the StarDust workspace (GNU linker on this host; `--features python` only if **this** binary was already built with python, so a python session stays incremental and a default session does not compile PyO3). Then `target\debug\fs-viz.exe --desktop --replace`. Running binaries are renamed aside so the build can overwrite them. A pinnable log window shows cargo output; on success the UI relaunches. Rebuild does **not** open a Windows File Explorer window (not Desktop, not any folder) as a side effect: while StarDust replace is armed, the folder trampoline / `open_native_folder` never falls back to Explorer if HTTP is down (desktop cover or `--open PATH` after `--desktop` instead). Guard yellow recovery (`taskkill` / failover) is held for cargo timeout + relaunch (~660s) and skipped during arm grace. `SHChangeNotify` (`SHCNE_ASSOCCHANGED`) runs only when HKCU folder verbs or HideIcons DWORD actually change — not when only hiding the desktop listview. Tray **Open Desktop in Explorer** and in-app **Open in Explorer** stay; Ctrl+Shift+F12 still restores Windows UI without opening Explorer. A rebuilt `--desktop` session is not ephemeral: it restores last views/split/pins and last 2D/3D star positions by directory path identity (not raw string equality); rebuild flushes layout first.

## Milestones

1. 2D cosmos, edit/save, pins, splits, agent, watch, search, breadcrumbs, persist.
2. Tools 2D ↔ 3D (orbit/dolly; fly-into then scene-swap; shared shell; separate positions).
3. Explorer mutate, Recycle Bin, copy/paste, OS drag-and-drop.
4. Inspect IR + Tools `cargo test` / Rebuild StarDust UI.
