# UI delta (current vs 3c2a4ae)

Baseline: current working tree vs commit 3c2a4ae.
Archive branch (after this file exists): archive/ui-peek-power is reference-only; do not copy that code forward.

Interview: yes / no / later, one item at a time.

Dropped as already in `3c2a4ae`: Sort menu itself; same-pane layout drag; `setPointerCapture` on the canvas; Shift+click spatial range; marquee; `GetLogicalDrives`; the **Searching more folders…** footer string; Inspect/CNS layout; language waves. Snapshot drive *code* did not peek-then-hide (that was the written contract only).

## Items

### 1. Brand wordmark chrome

- **Status:** yes (chrome only; click is item 2)
- **Name:** Centered luna + StarDust wordmark
- **Kind:** Add
- **Surfaces:** chrome
- **Now (current tree):** Top bar is menus | centered luna + **StarDust** wordmark | search in a fixed 220px right slot (does not cover the mark).
- **Then (3c2a4ae):** Menu bar then flex search (max 420px); no brand control.

### 2. Show Desktop

- **Status:** no
- **Name:** Show Desktop (wordmark / API)
- **Kind:** Add
- **Surfaces:** chrome / host
- **Now (current tree):** Wordmark click and `POST /api/show-desktop` inject Win+D. Cover stays the wallpaper (`SW_SHOWNA` when Progman/WorkerW is FG). Taskbar luna does nothing.
- **Then (3c2a4ae):** Absent.

### 3. Guard tray left-click

- **Status:** later
- **Name:** StarDust-UI tray left-click
- **Kind:** Replace
- **Surfaces:** chrome / host
- **Now (current tree):** Guard tray left-click in StarDust UI is Show Desktop (same as the wordmark). Menu **Open Desktop in Explorer** stays.
- **Then (3c2a4ae):** That left-click opened Desktop in Explorer.

### 4. Windows UI caption icon

- **Status:** yes
- **Name:** Black caption ico
- **Kind:** Add
- **Surfaces:** chrome / host
- **Now (current tree):** Floated Windows UI caption uses `stardust-caption.ico` (black ink, knockout). PE / taskbar stay white.
- **Then (3c2a4ae):** One white PE icon for caption and taskbar.

### 5. Sleep / lid wake

- **Status:** yes
- **Name:** Power-broadcast cover restore
- **Kind:** Add
- **Surfaces:** host
- **Now (current tree):** `WM_POWERBROADCAST` resume forces work-area cover restore plus WebView resume.
- **Then (3c2a4ae):** Absent; wake could leave the cover unrestored.

### 6. Ctrl+click multi-select

- **Status:** yes
- **Name:** Ctrl+click toggles selection
- **Kind:** Replace
- **Surfaces:** cosmos
- **Now (current tree):** Ctrl+click toggles a star in the selection. Ctrl+click empty does not clear. Enter/leave stay double-click, Enter, Alt+Up, Backspace.
- **Then (3c2a4ae):** Ctrl+click a directory entered it; Ctrl+click empty left to the parent.

### 7. Filesystem-move star drag

- **Status:** yes
- **Name:** Drop onto folder or other pane = MOVE
- **Kind:** Add
- **Surfaces:** cosmos / host
- **Now (current tree):** Drop a star (or the selection) on a **folder star** or on **empty space in another pane** moves those files. AppData / `stardust-ws-*` refused. Inspect/CNS panes do not move. Same-pane empty still only nudges layout.
- **Then (3c2a4ae):** Cross-pane empty drop **copied** (Shift moved). Drop onto a folder star was absent.

### 8. Live star-image drag

- **Status:** yes
- **Name:** Star image follows the pointer
- **Kind:** Add
- **Surfaces:** cosmos
- **Now (current tree):** The star image follows every move. Layout save is pointer-up (or a coalesced idle save). Window-level `pointercancel` keeps last coordinates for a cross-pane drop. Bring it back without dropping: layout resumes from the grab.
- **Then (3c2a4ae):** Layout drag existed; leaving the canvas cancelled the drag and did not keep a live ghost.

### 9. Sort as a row-major grid

- **Status:** yes
- **Name:** Sort grid in the current view
- **Kind:** Replace
- **Surfaces:** cosmos
- **Now (current tree):** View → Sort (Name / Type / Size) lays stars in a row-major grid centered in the **current view**.
- **Then (3c2a4ae):** Sort used a ring (few stars) or a long zig-zag row.

### 10. Double-click empty space leaves

- **Status:** yes
- **Name:** Empty double-click → parent
- **Kind:** Add
- **Surfaces:** cosmos
- **Now (current tree):** Double-click empty folder cosmos goes to the parent. Inspect neighborhood still leaves neighborhood and stays open.
- **Then (3c2a4ae):** Empty double-click did nothing in a folder cosmos (Inspect neighborhood already left).

### 11. Incremental search dropdown

- **Status:** yes
- **Name:** Streamed hits + selection look
- **Kind:** Add
- **Surfaces:** chrome / cosmos
- **Now (current tree):** Hits paint as they are found (`stream=1`). Hover or arrow uses the **click-selection look** on that star in every open view that shows it, or on the deepest visible ancestor folder.
- **Then (3c2a4ae):** Dropdown painted after each **root** finished (footer already said Searching more folders…). Hover highlighted only the parent directory.

### 12. Agent search, jump, attach

- **Status:** yes (search + jump; attach/@-chips declined)
- **Name:** search / jump / attach tools
- **Kind:** Add
- **Surfaces:** agent
- **Now (current tree):** Agent **search** (same ranking as the search bar) and **jump** (folder in-place; file → parent + select). Do not `open` a Windows viewer to ingest. Attach/@-chips declined.
- **Then (3c2a4ae):** Cosmos tools were look-at-view, navigate, open, select, pin, copy only.

### 13. Agent write-out directory

- **Status:** yes
- **Name:** write-out to .stardust-out
- **Kind:** Add
- **Surfaces:** agent / host
- **Now (current tree):** Generated payloads go in `%USERPROFILE%\.stardust-out` (`write-out`, `POST /api/agent-out/write`). Temporary screenshots live in `.stardust-out\cap-screen` and are deleted at run end (Desktop `cap-screen*` / `wa-view*` swept too).
- **Then (3c2a4ae):** Absent.

### 14. Delete chat stops sidecar runs

- **Status:** yes
- **Name:** stop-all before delete
- **Kind:** Add
- **Surfaces:** agent
- **Now (current tree):** History × (**Delete chat**) posts `/api/agent/stop-all` first so a removed thread cannot keep driving OS windows. **Stop** also hits stop-all.
- **Then (3c2a4ae):** Delete removed history only; Stop cancelled only the current thread.

### 15. Drive dock, leave open, undock

- **Status:** yes
- **Name:** Window-dock split
- **Kind:** Replace
- **Surfaces:** drive / agent / cosmos
- **Now (current tree):** First look/click/type/drop places the HWND in a reserved **window-dock** cosmos split, sizes it to that slot, and **leaves it visible**. User move off the slot **undocks**; later ops use the new rect (no snap-back, no hide). `parked` is only really iconic.
- **Then (3c2a4ae):** Snapshot code photographed/drove the HWND in place and restored previous FG. The drive doc said peek-then-hide; the binary did not dock or hide.

### 16. Cover click is wallpaper

- **Status:** no
- **Name:** Cover click must not raise
- **Kind:** Hygiene
- **Surfaces:** host
- **Now (current tree):** Clicking the work-area cover (cosmos, menus, Agent) must not hide, cloak, or raise StarDust over other apps. Cover click is not Show Desktop.
- **Then (3c2a4ae):** Cover activate/z-order could steal or cloak other windows.

### 17. Cover is not rude fullscreen

- **Status:** no
- **Name:** NonRudeHWND
- **Kind:** Hygiene
- **Surfaces:** host
- **Now (current tree):** `NonRudeHWND` on `StarDustHost` and WebView2 children before first ShowWindow. `WS_POPUP` only; `SC_MINIMIZE` swallowed so Win+D does not iconify the wallpaper.
- **Then (3c2a4ae):** Work-area popup could be treated as rude fullscreen (Explorer cloaking/minimizing Chrome).

### 18. Sky freeze rules

- **Status:** no
- **Name:** Sky freeze only hidden or other app
- **Kind:** Hygiene
- **Surfaces:** chrome / host
- **Now (current tree):** Sky, pane dust, and twinkle run while StarDust is in front, including after Show Desktop / Progman FG. They freeze for `document.hidden` or another **user** app FG. Taskbar FG must not pause/resume. WebView stays visible.
- **Then (3c2a4ae):** Activate/hide and taskbar FG could strobe or freeze the sky incorrectly.

### 19. IME / RIME while composing

- **Status:** yes
- **Name:** IME freeze
- **Kind:** Hygiene
- **Surfaces:** chrome / host
- **Now (current tree):** Search does not query mid-composition (keyCode 229 / `isComposing` holds until `compositionend`, search blur, or a non-229 key; 700ms quiet after `compositionend` then search). Host does not ExecuteScript / MoveFocus / apply bounds / pin z-order while composing (`POST /api/ime`) or when the foreground/insert-after class looks like Weasel/TSF IME UI.
- **Then (3c2a4ae):** Composition often killed candidates or fired search between syllables.

### 20. View-mode window restyle

- **Status:** no
- **Name:** FLOAT and SYNC_BOUNDS
- **Kind:** Hygiene
- **Surfaces:** host
- **Now (current tree):** View → Windows UI posts `WM_STARDUST_FLOAT`. View → StarDust UI posts `WM_STARDUST_SYNC_BOUNDS` so WebView `put_Bounds` matches the work-area HWND.
- **Then (3c2a4ae):** Mode switch could leave the WebView at float size while the HWND was a work-area cover.

### 21. Open in Cursor from the view thread

- **Status:** yes
- **Name:** Open in Cursor z-order yield
- **Kind:** Hygiene
- **Surfaces:** chrome / host
- **Now (current tree):** Tools → Open in Cursor posts to `StarDustHost`. The view launches Cursor and yields cover z-order for a few seconds.
- **Then (3c2a4ae):** `ShellExecute` ran from the HTTP worker; Cursor often returned ok with no visible window.

### 22. HTTP hang avoidance

- **Status:** later
- **Name:** Listing / inbox / pulse during drag
- **Kind:** Hygiene
- **Surfaces:** host / agent
- **Now (current tree):** `/api/children` does not open files to sniff text. Agent UI inbox long-polls (`?wait=2500`). UI pulse ~2.5s live / ~4s idle and does not stack. Health poll skips while a star is dragged. Folder watch waits for pointer-up if that pane is mid-drag.
- **Then (3c2a4ae):** Folder listing probed each file for text; inbox was a 200ms GET hammer; pulse/health ran during drag.

### 23. `--open` stays desktop cover in StarDust mode

- **Status:** yes
- **Name:** --open while StarDust mode
- **Kind:** Hygiene
- **Surfaces:** host
- **Now (current tree):** `fs-viz --open` while mode is StarDust still loads `?desktop=1` (work-area cover).
- **Then (3c2a4ae):** `--open` could show a captioned/floated view while StarDust UI mode was still on.
