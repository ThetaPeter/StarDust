# Desktop drive

Agent note: drive a **chosen OS window** from StarDust. Not a language wave. Not [waves.md](waves.md). Not the chrome contract ([StarDust UI.md](StarDust%20UI.md)) — this stays a **new file**.

Tools are **generic**. They must work for any normal app window (browser, messenger, editor, Explorer, installer). Do not special-case WhatsApp in the API.

## Locked

| Item | Decision |
| --- | --- |
| Where this lives | This file. Do not fold into [problems.md](problems.md) or [StarDust UI.md](StarDust%20UI.md). |
| How much the agent may see | **All:** focused OS window, work area, and full desktop (every monitor). |
| Contract file | Keep the tool list here until you say otherwise. |
| Target app | Not important. No WhatsApp-only path. |
| Input | **Never** move the hardware cursor (`SetCursorPos` / `SendInput` mouse). Drive a chosen HWND. |
| Foreground | **Dock, then leave open.** Raise only for `look-at-window` / `window-click` / `window-type` / `drop-file` (and those steps inside `run-ui`). Size into the reserved window-dock cosmos pane and **leave the app visible**. Do not hide or minimize when the call returns. If the user switched apps during the burst, do not yank focus back. Never `SetForegroundWindow(StarDustHost)`. |
| Peek slot | Driven window covers the **focused cosmos** (or an existing `windowDock` pane if the layout still has one). Never the Agent panel. Do **not** auto-split a second Desktop on boot or Refresh web. Drag the OS window off the slot (**> 16px** in x or y) to **undock** — later ops keep the user’s size (no snap-back). |
| Looks to the model | One final JPEG if OK, or the failure JPEG if stopped. Midpoint JPEG only when the next click would be a guess (`look-*` in `run-ui`). |
| Pacing | One `run-ui` ≈ a simple task (about 12–20 host steps). Not one click per chat turn. |
| Attach | `drop-file`: absolute host path onto the target (`WM_DROPFILES` / `CF_HDROP`). Generic, not an app API. |
| Cover z-order | Drive code must not `HWND_TOP` / `HWND_TOPMOST` / `HWND_NOTOPMOST` on `StarDustHost`. Peek is not TOPMOST. `HWND_TOP` on the **driven** HWND only is OK. Never steal z-order over the Agent chrome. |
| `exists` | Host HTTP `GET /api/exists?path=` (not inbox). Missing → `{ exists: false }` (not 404). If present: `{ exists: true, kind: "file"\|"dir", children? }`. Immediate child **names** only, cap ~40, no recurse. Refuse AppData / `stardust-ws-*`. Does not change the focused cosmos. |
| `mkdir` | `POST /api/mkdir` wraps leaf `create_dir`. Parent must exist. No `create_dir_all`. If parent is missing, error (agent asks). Same AppData / agent-temp reject family as move. |
| `search` | Sidecar `search` is host HTTP `GET /api/search?root=&q=` (not inbox). Optional `root`; default `sessionRoot` / `desktopPath` / `defaultRoot` from `GET /api/meta`. Few-second timeout. Miss → empty / "No matches". Does not change the focused pane. One query per call. |
| Inbox | `GET /api/ui/inbox` takes **one** command per poll; the rest stay queued. Do not drain the batch. |
| `open` | Names in the focused cosmos: unchanged. Absolute **file or `.lnk`**: launch (`open_external`) even if not a star; **do not change** the cosmos folder. Absolute **dir**: `jump`, not Explorer. Refuse AppData / `stardust-ws-*`. |
| `list-windows` | Include `processName` (exe stem; empty if unknown). Optional `url` only from the window title (strip ` - Google Chrome` / ` - Microsoft Edge` / ` - Mozilla Firefox`; leftover must not be just the browser name). Never AppData / History. Keep iconic `parked` rows. Empty-title skip stays. |
| `look-at-window` | Stays **outside** `run-ui`. |

Still later: per-app Share/Send verbs; a second Windows desktop / nested session.

Hard rules: never move, rename, copy-away, relocate, or amend Windows AppData (Roaming, Local, or LocalLow), including `%APPDATA%\StarDust`. Do not Write, Shell, copy, or delete under those trees. Never target `stardust-ws-*`. Do not leave leftover screenshot files on Desktop unless asked. Never move the hardware cursor. `look-at-view` is cosmos only. Do not drive UI via Shell. Do not read Chrome/Edge History or session files for a URL.

## Dock, then leave open

Host implements dock/leave-open. Agents do not Shell-minimize, hide, or `HWND_TOPMOST`.

Peek-slot is the focused cosmos `.vcosmos` (or a saved `windowDock` pane). JS posts `POST /api/drive` `{ op: "peek-slot", args: { x, y, w, h } }`. Never auto-create an extra Desktop split on Refresh.

| Op | Dock? |
| --- | --- |
| `look-at-window` | Yes. Leave visible in the slot (or the user’s undocked rect). |
| `window-click` / `window-type` / `drop-file` | Yes. Leave visible when the op finishes. |
| `run-ui` | One raise/dock for the HWND burst; **leave visible when the burst ends**. Do not peek-finish between steps. |
| `look-at-view` / `navigate` / `open` / `select` / `pin` / `copy` | **No.** Driven app stays where it is. |
| `list-windows` / `target-window` | **No raise.** Bind pid/id without moving the window. |
| `look-at-workarea` / `look-at-desktop` | **No raise** of the driven app. |
| `exists` / `mkdir` / `search` | **No.** Host HTTP; focused cosmos stays. |

`open App.lnk` is a user-visible launch. After first bind, further looks are host dock-into-slot — not another shortcut open. `list-windows` first if a usable HWND may already exist.

Size the driven window to the cosmos slot (**shrink if larger; do not enlarge** a smaller window). Re-read the slot if the dock pane resizes.

**Drag = undock:** if the window origin leaves the slot by **> 16px** in x **or** y, mark undocked. Resize in place (origin still near the slot) stays docked. Later ops must not snap back. Keep the user’s size after undock. Closing the dock split does not move an undocked HWND.

`list-windows` `parked` is **only** `IsIconic` (user/OS minimize) — not a host hide. Include iconic parked rows so the next look can restore. Title-only still rejects cloaked/tiny strips (`unusable`); pid/id may still bind those.

Agent must not: re-open an app shortcut between looks; Shell-minimize or hide between looks; use work-area/desktop look as a substitute for `look-at-window`.

## Tools (shipped)

### Drive — `POST /api/drive` (Rust)

Do **not** go through the cosmos JS inbox.

| Tool | Job |
| --- | --- |
| `list-windows` | Top-level windows: title, `processName`, pid, hwnd/id, bounds, client size, `parked` (iconic only). Optional `url` from a browser-suffixed title only. `unusable` when origin is `±32000`, DWM-cloaked, or client ≲ 200×40. |
| `look-at-window` | JPEG of the target OS window. Pixel size, numbered 100px grid, UIA hit-list (name + box in JPEG space). Max edge 1600. **Outside** `run-ui`. |
| `look-at-workarea` | JPEG of the primary work area. |
| `look-at-desktop` | JPEG of the full desktop (every monitor). |
| `target-window` | Bind by `pid` and/or `id`. Several usable title matches and no pid/id → error with the candidate list. Title-only never binds a cloaked strip. |
| `window-click` | Click in last-look bitmap coordinates or by control name. UIA invoke first. Never `SetCursorPos`. A coordinate click off a hit box that does not change the UI is a **miss**. |
| `window-type` | Type text and chords into the target. Chords: Tab, Shift+Tab, Ctrl+A, Ctrl+L, Backspace, Delete, Escape, Enter, Ctrl+V, Ctrl+F, plus Ctrl+Enter / Ctrl+letter. |
| `drop-file` | Drop an absolute host path (`CF_HDROP`). |
| `run-ui` | Ordered list of the above except `look-at-window` (max ~20). Stops on miss / drop failure / new dialog. Returns the final or failure JPEG. |

`run-ui` / `window-type` refuse typing or Enter when `contact` / `header` / `expect` is set unless the last look’s title or a hit name contains that string, or `force: true`. Never PostMessage into `StarDustHost`.

### Cosmos / host files — HTTP (not inbox for search/exists/mkdir)

| Tool | Job |
| --- | --- |
| `exists` | `GET /api/exists?path=` → `{ exists, kind, children? }`. No navigate. |
| `mkdir` | `POST /api/mkdir` `{ path }`. Parent must exist. Refuse AppData / `stardust-ws-*`. |
| `search` | `GET /api/search?root=&q=`. Sidecar tool; not `runUiTool("search")`. |
| `open` | Names in focused cosmos unchanged. Absolute file/`.lnk` → `POST /api/open-external` without a star. Absolute dir → `jump`. |
| `jump` | Folder opens in the focused pane. File → parent + select. |

## Agent must not

- Shell UI automation (`Add-Type`, `mouse_event`, `SendKeys`, `CopyFromScreen`)
- `Test-Path` / Shell existence checks (use `exists`)
- Screenshot files on Desktop
- `HWND_TOPMOST` on other apps or `StarDustHost`
- One click per turn when the next click depends on the last screenshot
- Moving the user’s real cursor
- `drop-file` until the look shows the **named** thread, not the first chat or a splash pane
- Pack `look-at-window` inside `run-ui` (looks time out; look **outside** the burst)
- Trust image-caption coordinates over the tool-printed `N@x,y` cells
- Read AppData, Chrome/Edge History, or session files for a URL
- Launch Chrome (or another app) before `list-windows` when a usable HWND may already exist
- Open an absolute directory with `open` / Explorer (`jump` instead)

## Playbook (generic attach)

```
look-at-view + list-windows
exists { absolute host path }       # do not navigate; never AppData
search { q }                        # host HTTP; miss is No matches
mkdir { absolute dir }              # parent must exist; ask if missing
jump { absolute dir }               # not Explorer; not open
open App.lnk only if never bound     # absolute .lnk is ok from any cosmos folder
list-windows                         # title + processName; optional url from title
target-window pid+id               # inner client, not frame
look-at-window                      # dock into window-dock, leave visible; outside run-ui
window-click contact row
window-click composer
drop-file {absolute host path}
look-at-window                      # filename staged + named header
window-click send control           # do not Enter+contact on Chromium
look-at-window                      # confirm in history
```

Prefer `look-at-window` over work-area/desktop while driving. After bind, the host docks into the window-dock slot rather than re-opening a shortcut. Cosmos work uses `look-at-view`; an undocked or iconic parked app stays put until the next look.

New-app first run (Chrome profile picker, etc.): bind the HWND, `look-at-window`, then `window-click` the named profile. Do not assume the last URL is already open. URL is the visible title leftover / address-bar hits only.

## Host leftovers

1. Outer Electron/WinUI frame `look-at-window` can time out — bind the largest usable child client.
2. Chromium hit-list is often one `Chrome Legacy Window` blob (no named chat rows).
3. `contact` sees title/hits, not JPEG header text.
4. `drop-file` `ok` does not always mean the file staged.
5. Unminimize: `unusable` at ±32000; restore is a cosmos-slot dock, then leave visible — not a floating work-area window, and not a hide.
6. After the window dies, `list-windows` cannot recover the last URL. Process list is “binary running,” not “what page.”

Parked: G1 (real cursor) is **rejected**. Per-app Share/Send is later.

## Lessons (09-09)

- Existence without navigating: `exists` or host `search` (few-second miss), not `Test-Path` and not a cosmos walk.
- `mkdir` is a tool; parent must already exist.
- Absolute `.lnk` / file `open` must not require a star in the focused folder; absolute dirs are `jump`.
- Inbox is one command per poll so parallel cosmos ops do not all hang.
- Site/task: `list-windows` (`processName` + title `url`) then `look-at-window`. No AppData History.
- `look-at-window` stays outside `run-ui`.
