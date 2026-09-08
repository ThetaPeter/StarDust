# Desktop drive

Agent note: drive a **chosen OS window** from StarDust. Not a language wave. Not the executable book ([waves.md](waves.md)). Not the UI chrome contract ([StarDust UI.md](StarDust%20UI.md)) — this stays a **new file**.

Motivating failure: send a host PDF to a WhatsApp contact. The tools below are **general purpose**. They must work for any normal app window (browser, messenger, editor, Explorer, installer), not a WhatsApp-only path.

## Locked

| Item | Decision |
| --- | --- |
| Where this lives | New file: this document. Do not fold into [problems.md](problems.md) or [StarDust UI.md](StarDust%20UI.md). |
| How much the agent may see | **All:** focused OS window, work area, and full desktop (every monitor). |
| Contract file | Keep the tool list here. Do not add desktop-drive to [StarDust UI.md](StarDust%20UI.md) until you say so. |
| Target app | Not important. Do not special-case WhatsApp Web vs Desktop. |
| Which file to attach in that old task | Not important. |
| Input | **Never** move the hardware cursor (`SetCursorPos` / `SendInput` mouse). Drive a chosen HWND. |
| Foreground | **Peek then hide.** Raise the target only for `look-at-window` / `window-click` / `window-type` / `drop-file` (and those steps inside `run-ui`). When the call returns, **hide it again** (minimize or hide — not a cloaked 199×34 sink the next peek cannot find). Do not leave the app sitting on the work area between calls. Save/restore the user’s foreground; if they switched apps during the burst, do not yank focus back. |
| Peek slot | When shown, the driven window **covers the StarDust cosmos view only** (focused pane, or the cosmos column). It must **not** cover the Agent panel (header, transcript, composer). User monitors the agent while a look/click runs. If Agent is collapsed, the slot may use that width. |
| Looks to the model | Host may snapshot after each step to decide stop/continue. The agent sees **one final JPEG if OK**, or **the failure JPEG if stopped**. Midpoint JPEG only when the next click would be a guess (`look-*` in `run-ui`). |
| Pacing | One `run-ui` ≈ a simple task (about 12–20 host steps). Not one click per chat turn. Not a 30+ unattended script. |
| Attach | `drop-file`: absolute host path onto the target window (WM_DROPFILES / CF_HDROP). Generic, not an app API. |
| Cover z-order | Drive code must not `HWND_TOP` / `HWND_TOPMOST` / `HWND_NOTOPMOST` on `StarDustHost`. Cover pin must not insert after a user app. Peek is **not** TOPMOST: place the driven HWND in the cosmos rect above the cover’s view pixels only. Never steal z-order over the Agent chrome. |

Still later (not this file’s job): per-app Share/Send verbs; a second Windows desktop / nested session.

Hard rules (unchanged): never write/copy/delete under AppData (Roaming / Local / LocalLow), including `%APPDATA%\StarDust`. Never target `stardust-ws-*`. Do not leave leftover screenshot files on Desktop unless asked. Never move the hardware cursor. Keep `look-at-view` for cosmos. Do not reuse it for other apps. Do not drive UI via Shell.

## Peek over cosmos, hide otherwise (2026-09-08)

Motivating failure: WhatsApp (and the same class of driven apps) **kept flowing** — stayed up or re-raised on every look/click — and sat at ~1000×1028 covering **both** the cosmos and the Agent. The user could not work in other apps or read the Agent transcript.

This is **generic** (locked: do not special-case WhatsApp in the API). Host implements peek/hide. Agents do not Shell-minimize or `HWND_TOPMOST`.

### When the window may be visible

| Op | Peek? |
| --- | --- |
| `look-at-window` | Yes — need pixels. Hide when the JPEG is captured. |
| `window-click` / `window-type` / `drop-file` | Yes — need the HWND on-screen in the peek slot. Hide when the op finishes. |
| `run-ui` | Peek for the whole burst that needs the HWND; **hide once when the burst ends** (success or stop). Do not hide between two clicks in the same `run-ui` if that would race the UI. |
| `look-at-view` / `navigate` / `open` / `select` / `pin` / `copy` | **No.** Cosmos only. Driven app stays hidden. |
| `list-windows` / `target-window` | **No raise.** Bind pid/id while hidden. `target-window` still does not leave the app focused (existing). |
| `look-at-workarea` / `look-at-desktop` | **No raise** of the driven app. Do not unhide WhatsApp just to photograph the desk. |

`open App.lnk` from cosmos is a **user-visible launch**. After first bind, further peeks are host raise-into-slot, not another shortcut open (that is what left the messenger floating on 09-08).

### Where it may cover

- **Yes:** focused cosmos canvas (same region `look-at-view` shoots — not menus, not Agent).
- **No:** Agent panel (tabs, History, transcript, composer, Stop). The user watches the agent **during** a peek.
- **No:** other user apps the user is working in. Peek is not a work-area or full-desktop takeover.
- Size the driven window to the cosmos slot (shrink if it is larger). Do not leave it at the last casual `@540,0` 1000×1028.
- Agent collapsed → slot may grow into that width. Agent opened mid-burst → next peek must re-read the slot (do not stay over the composer).

### Hide / park

After peek: hide or minimize **without** turning the HWND into an `unusable` −32000 strip the next `target-window` cannot bind. `list-windows` should still return **pid / id** (e.g. `parked: true`) so the next look can pop it again.

Do not `HWND_TOPMOST`. Do not z-order `StarDustHost`. Do not pin the cover after the user app.

### Agent must stop doing

- Re-`open`ing `WhatsApp.lnk` (or any app shortcut) between looks “to keep it up”
- Leaving the messenger visible while writing files, navigating cosmos, or thinking
- Using `look-at-workarea` / `look-at-desktop` as a substitute for `look-at-window` (that keeps the app in the user’s face)
- Asking the user to live with a floating chat over the Agent

## What went wrong (2026-09-07 run)

The first-party tools in §A already exist. This run used them only (`look-at-view` / `navigate` / `open` / `select`, then `list-windows` / `target-window` / `look-*` / `window-click` / `window-type` / `run-ui`). No Shell mouse, no Desktop screenshot file, no `HWND_TOPMOST`.

The host file was found. The send never happened.

### Found, then stuck

Cosmos path that worked:

1. Desktop → `ETH` → `- MP` (Machine Perception).
2. Lecture `2025-MP-l09a-DM.pdf` (`C:\Users\Peter\Desktop\ETH\- MP\2025-MP-l09a-DM.pdf`). `ex9_dm_instructions.pdf` / `2025-MP-l09b-DMFM.pdf` are siblings; the ask was singular.
3. `WhatsApp.lnk` on Desktop launched the store/desktop app.
4. Chrome profile **Peter** opened `web.whatsapp.com` and was already logged in. **Francis** was the fourth chat. Preview already showed `2025-MP-l09a-DM.pdf` at 17:06.
5. `drop-file` was never safe to call: Francis’s thread was never the target.

### 1. `target-window` is title-substring only

`list-windows` in that run:

- `WhatsApp` ~1000×1028 @ (540,0) — desktop app, **QR login**
- `WhatsApp` ~982×1018 nearby — second desktop HWND
- later, a third `WhatsApp` at **(-32000, -32000)** size **199×34** (minimized / cloaked)
- Chrome tab title `(3) WhatsApp` / `(4) WhatsApp`

`target-window title: WhatsApp` does not take pid, bounds, or z-order. It bound the **wrong HWND**.

`look-at-window` then returned the 199×34 strip (non-client sink only). Later clicks meant for Chrome’s chat list went to the desktop login window. The Chrome JPEG did not change, so the agent kept re-aiming Francis on a picture that was not the click target.

Substring `"WhatsApp"` also matches Chrome. The agent cannot say “largest visible” or “exclude origin -32000.”

### 2. Hit-list never reaches web / Electron content

Looks advertised a coarse grid and a **named control hit-list**. What actually came back:

| Window | Hits |
| --- | --- |
| WhatsApp desktop login | `Non Client Input Sink Window`, `ReunionCaptionControlsWindow` (caption only) |
| Chrome (profile picker, New Tab, WhatsApp Web) | `Chrome Legacy Window@9,108` (or `@9,151`) — one client blob |

No hit named Francis, 搜尋, 傳送文件, 使用電話號碼登入, 登入, paperclip, or profile **Peter**. UIA invoke-by-name was unusable. Every act was a bitmap guess.

The phone-number login link on the QR page was clicked at y ≈ 500, 560, 620, 710, 760, 800, 880. The page never changed.

### 3. The model does not get clickable coordinates

`look-*` returns a JPEG. The agent is supposed to click `(x,y)` from that bitmap. In practice the model sees a **prose caption** of the JPEG, not stable grid numbers.

Captions disagreed:

- Francis as “fourth row, about halfway down the sidebar”
- Francis as `[650, 250]` on a 0–1000 scale (that `x` is the **chat pane**, not the list)
- Francis as `(x=250, y=670)` on another 1000×1000 guess
- An A–T / 1–22 letter grid on one frame, then gone

Work-area JPEG was 1600×850 while the monitor was 1920×1080 and Chrome was 1006×1029. Mapping “fourth chat” through three spaces missed.

This is the same class of error the old Shell run had (“guessed click points from a prose image description”). First-party looks did not close it.

### 4. One missed click is not a miss

`run-ui` “stops on miss / new dialog.” A coordinate click on empty chrome is a **hit**. The host returned OK and a JPEG of the same (or worse) UI.

What those “hits” did:

- Left-rail click opened **已封存 (Archived)** instead of Chats. Francis is not in that list. The back arrow / Chats icon then missed for several turns.
- Clicks meant for the WhatsApp search box opened **MATH Course B range** (first chat).
- `window-type text: Francis` after a missed search click landed in that group’s composer (`Francisf` after Ctrl+F + type). Escape did not clear it. A green send button appeared. `Ctrl+A` and `Backspace` are **unsupported chords**, so the draft could not be deleted from the tool.
- Archive / first-chat / composer mistakes burned the 12–20 step budget. The agent fell back to one click + look per turn — the pacing this file already forbids.

`drop-file` on the wrong thread would have sent the ETH lecture to a course group. The run stopped short of that.

### 5. Chord set is too small for real apps

Supported in the tool text: `Enter`, `Escape`, `Ctrl+V`, `Ctrl+F`, and an ellipsis.

Used and **rejected**: `Tab`, `Ctrl+A`, `Backspace`.

Needed and never available as a reliable chord: `Ctrl+L` (Chrome omnibox), `Delete`, `Ctrl+Enter`.

Tab was the only way to reach “使用電話號碼登入” without a hit-list. Clearing the accidental MATH draft needed select-all + delete. Both failed.

### 6. Login and dual surfaces

Desktop WhatsApp opened **logged out** (QR + “使用電話號碼登入”). A QR scan is not an agent step. The phone-number path was invisible to UIA and unhittable by guessed `y`.

Chrome WhatsApp Web was already logged in on the same desktop. The tools have no “prefer the logged-in surface” signal. The agent had to discover Web by opening Chrome, picking a profile, and typing `web.whatsapp.com` into the **page** search box after address-bar clicks missed.

Two live messengers with the same product name is normal. Title-substring targeting is not enough.

### 7. `run-ui` could not be one task

The locked pacing says one `run-ui` ≈ “open contact, attach file, send.” That needs either named hits or a labeled grid the model can read.

This run was: look → guess → look → guess, plus retarget after the wrong HWND, plus recover from Archive and a dirty composer. `drop-file` never ran. Francis never opened.

## Ways to resolve

The tool **names** in §A stay. The host behavior does not.

### A. First-party desktop-drive tools (shipped; keep)

Expose these as StarDust / Cursor **custom tools**. Host HTTP `/api/drive` (Rust). **Do not** go through the cosmos JS inbox (`look-at-view` / `window.SD`). Do **not** implement the drive path as ad-hoc Shell.

| Tool | Job |
| --- | --- |
| `list-windows` | Visible top-level windows: title, pid, bounds. |
| `look-at-window` | JPEG of the **target OS window** (or a window chosen by title). Not cosmos-only. Pixel size, coarse grid, **UIA / child-window hit-list** (name + box). Origin top-left of that bitmap. |
| `look-at-workarea` | JPEG of the primary work area. Same grid / size / origin. |
| `look-at-desktop` | JPEG of the **full desktop** (every monitor). Same grid. State origin and each display’s rectangle. |
| `target-window` | Select HWND by title substring for later look/click/drop. Does **not** leave it focused. |
| `window-click` | Click in **bitmap coordinates** from the last look, or by control name. UIA invoke first; window messages fallback. Brief activate only if needed; restore. Never `SetCursorPos`. |
| `window-type` | Type text and chords (`Ctrl+V`, `Enter`, `Ctrl+F`) into the **target** window. Brief activate only if needed; restore. |
| `drop-file` | Drop an **absolute host path** onto the target window (`CF_HDROP` / `WM_DROPFILES`). Generic attach, not an app-specific API. |
| `run-ui` | **One call**: an ordered list of the above (max ~20 steps). Host stops on miss / drop failure / new dialog. Returns the final JPEG, or the failure JPEG. |

### B. Make targeting unambiguous (shipped)

- `list-windows` returns **pid, hwnd/id, outer bounds, visible client size**, and **`unusable`** when origin is `±32000`, the window is DWM-cloaked, or client size is ≲ 200×40.
- `target-window` takes `pid` and/or `id`/`hwnd`, not only a title substring. Several **usable** title matches and no pid/id → return the candidate list and **error** (do not pick in silence). After dropping cloaked/tiny, a **single** remaining match may bind. Title-only never binds a cloaked strip.

### C. Make the look actually clickable (shipped)

- Hit-list walks **UI Automation** (UiaNodeFromHandle / UiaFind / tree walk) for Chrome / Edge / Electron, not native caption chrome only. Names + boxes are in **the same bitmap space as `window-click`**.
- Overlay a **numbered 100 px grid** on the JPEG. Tool result text prints those cell numbers and the hit boxes.
- `window-click name:` matches **visible-text / a11y-name substring** (UIA).
- A coordinate click that does not change the target UI, and is not on a hit-list box, is a **miss**. `run-ui` stops and returns that JPEG.
- **Scale** bitmap↔screen the same way hit boxes are scaled (capture shrinks to max edge 1600).

### D. Chords and composer safety (shipped)

Minimum chords: `Tab`, `Shift+Tab`, `Ctrl+A`, `Ctrl+L`, `Backspace`, `Delete`, `Escape`, `Enter`, `Ctrl+V`, `Ctrl+F` (plus `Ctrl+Enter` / `Ctrl+letter`).

`run-ui` / `window-type` refuse typing or Enter when `contact` / `header` / `expect` is set unless the last look’s title or a hit name contains that string, or the agent set `force: true`. If those fields are omitted, generic apps are not blocked. Never PostMessage into `StarDustHost`.

### E. What the agent must still stop doing

- Shell UI automation (`Add-Type`, `mouse_event`, `SendKeys`, `CopyFromScreen`)
- Writing side-channel screenshots onto Desktop (`wa-view.png` and the like)
- `HWND_TOPMOST` on other apps or `StarDustHost`
- One click per turn when the next click depends on the last screenshot — **once C lands**; until then, guessed `(x,y)` will keep failing this class of task
- Moving the user’s real cursor
- `drop-file` onto a messenger until the look shows the **named** thread, not the first chat or a splash pane

## Open: generic drive vs app shortcuts

Parked. G1 (real cursor) is **rejected**. Per-app Share/Send verbs are later. This file ships window-scoped drive only.

Ambiguous same-title HWNDs and web hit-lists were the 2026-09-07 blockers; B–D above is the host fix. Live Chrome/WhatsApp names were not re-exercised against a real HWND in that landing.

## Next

B–D shipped (targeting by pid/id, UIA hits, numbered 100px grid, miss-on-no-change, extra chords, composer `contact`/`header`/`expect` + `force`). Do not retry desktop tasks via Shell.

`drop-file` onto a messenger still requires the look to show the **named** thread, not the first chat or a splash pane (§E).

## 2026-09-08 runs (two sends succeeded)

Same first-party tools. No Shell mouse, no Desktop screenshot file, no `HWND_TOPMOST`, no writes under AppData / `stardust-ws-*`.

| Task | Host file | Thread | Outcome |
| --- | --- | --- | --- |
| MATH3030 HW1 | `C:\Users\Peter\Desktop\CU\Year 5 Sem 1\MATH3030\26-27_T1_MATH3030_HW1.pdf` | Songtianze Huang | Sent 07:32 |
| AI course pack (plan + notes + links) | `C:\Users\Peter\Desktop\Ben-AI-Course-Materials-Sep-Dec-2026.md` | Ben | Sent 07:46 |

B–D (pid/id target, 100px grid, miss-on-no-change, extra chords, `contact` guard) are live. The send path **works**. It was still slow: many turns were look → guess → look, `run-ui`+`look-at-window` **timed out**, and UIA still does not name chats or buttons.

### What actually worked

1. **Cosmos first, then drive.** `look-at-view` → `navigate`/`open` for the file. `list-windows` in **parallel**. Do not hunt the PDF inside WhatsApp.
2. **Bind the content HWND, not the frame.** `list-windows` showed two usable WhatsApp surfaces: frame ~1000×1028 (pid 4436) and inner client ~982×1018 (pid 13264). `look-at-window` on the **frame timed out**. `target-window` **pid + id** on the inner window returned a usable JPEG.
3. **Unminimize via cosmos `open` on `WhatsApp.lnk`.** After minimize, the app is `unusable` at (−32000,−32000). Taskbar clicks on a **work-area** JPEG miss the target HWND (wrong coordinate space). Re-open the Desktop shortcut; then `list-windows` again.
4. **One last-look space only.** Clicks must be in the bitmap of the last `look-*`. Work-area `(850,500)` while the target is the WhatsApp client is a miss. Never convert “grid 73 on the desktop cover” into a `window-click`.
5. **Trust the tool-printed 100px cells, not the caption’s invented points.** Captions said Continue at `[701,513]`, `{70,50}`, or “cell 65.” The printed map is `76@500,700`. The green 繼續 that dismissed What’s New was ~**(520, 740)** (window-center x, lower third) — not the conversation-pane center (~680).
6. **Wait on splash, do not spray y.** Welcome → 登入, then QR, then “正在載入你的對話”, then What’s New. After a click that *might* have landed, **look once after a few seconds**. Eight guessed login y’s (09-07) and three Continue guesses (09-08) were the same waste.
7. **Confirm the header in the JPEG, then attach.** Songtianze / Ben were already in the left list. Click the **row** (sidebar x ~250, y from the 100px row), look, then drop. Search/`window-type` is optional and fights the composer guard.
8. **Focus the composer, then `drop-file`.** A drop onto an unfocused webview often returns `ok` with **no** staged file. Click the “輸入訊息” bar, drop the **absolute Desktop path**, look for the filename + green send plane.
9. **Send with `window-click` on the plane, not Enter.** Title stays `(1) WhatsApp`. Hits stay `Chrome Legacy Window` / 網頁內容. `window-type` + `contact: Songtianze` **refuses** (title/hits do not contain the name). The JPEG header did. Click ~**(930, 960)** (cell 110). Do not set `force` unless the look is ambiguous.
10. **Write the payload on the real Desktop**, then one attach. Course pack as one `.md`. Do not Write onto the `stardust-ws-*` cwd floor.

### Where time went (avoid next time)

| Waste | Faster move |
| --- | --- |
| `run-ui` that includes `look-at-window` | Timed out. Look **outside** `run-ui`. Use `run-ui` for `target-window` + click + `drop-file` only, or click→look as two host calls. |
| One click per turn after that | Pack **known** geometry: `window-click` composer → `drop-file` → (separate) `look-at-window` → `window-click` send. That is one attach, not four chat turns of doubt. |
| Outer HWND / title-only `WhatsApp` | Always `list-windows` → pick **largest usable client** with pid+id. Skip `unusable`. |
| Mixing work-area / desktop / window pixels | After retarget, **only** `look-at-window` until send completes. |
| Caption “grid 73/89” and `{x,y}` on a 0–100 scale | Use `N@x,y` from the tool. If the caption and the printed cell disagree, **printed cell wins**. |
| Hunting contacts with search | If the JPEG already lists the name, click the row. Search only when it is not in the first screen. |
| Enter + `contact` on Chromium messengers | Will refuse. Click send. |
| Re-login / QR rabbit hole | If chats start loading, stop clicking 登入. |
| What’s New / other center modals | Assume modal is **window-centered**, CTA in the **lower-middle** of the card. One look after click. |
| Deep cosmos walk | Course codes live under `CU\Year * Sem *\<CODE>`. Open that path; do not browse every year. |

### Agent playbook (messenger attach, ~6 host calls)

```
look-at-view + list-windows          # parallel; messenger should stay hidden
navigate/open to the file            # cosmos; remember absolute path
open App.lnk only if never bound    # first launch only — not between looks
list-windows
target-window pid+id                 # inner content HWND, not frame; no raise
look-at-window                       # host peeks over cosmos only, then hides
window-click contact row             # peek → click → hide
window-click composer
drop-file {absolute host path}
look-at-window                       # filename + send plane + named header
window-click send plane              # do not Enter+contact
look-at-window                       # filename in history, plane gone → done
# host hides; Agent stayed visible the whole time
```

Insert `look-*` **only** when the next click would be a guess (locked pacing). Do not `drop-file` until the JPEG header is the named thread (§E). Prefer one `run-ui` burst so peek stays up for the clicks and hides **once** at the end.

Prefer `look-at-window` over work-area/desktop while driving. Cosmos work uses `look-at-view` with the app **parked**.

## Still slow — host leftovers (after B–D)

These are why 09-08 still took many turns even though the send landed.

1. **`look-at-window` on the outer Electron/WinUI frame times out.** Prefer auto-bind the largest *child* client, or fail fast (“use pid of the 982×1018 surface”).
2. **`run-ui` + `look-at-window` times out.** Either make look cheap enough for a 20-step burst, or document “look is a separate call” so agents stop retrying the packed form.
3. **Hit-list is still one `Chrome Legacy Window` blob** (same as 09-07 §2). C is marked shipped; Chromium WhatsApp still has no named 登入 / 繼續 / chat row / paperclip. Until UIA or an a11y tree reaches web content, the model will keep guessing from captions.
4. **Composer `contact` cannot see the JPEG header.** Guard only checks title + hit names. For web messengers those never contain “Ben”. Add OCR / visible-text lines from the header strip to the look payload, or treat a caption/header match as enough without `force`.
5. **Image captions invent coordinates.** The model does not see raw pixels. Captions must quote the **printed 100px cells** (`76@500,700`) and must not emit a second grid (`{70,50}`, “row 95 col 4.7”).
6. **`drop-file` `ok` ≠ staged.** Host should miss (or return a flag) when CF_HDROP does not change the target UI within N ms — same rule as coordinate clicks.
7. **Unminimize.** `list-windows` already marks `unusable`. A `target-window` restore (show the HWND without `HWND_TOPMOST`) would skip the `.lnk` / taskbar dance — but restore must be a **cosmos-slot peek**, then hide (see Peek section). Do not restore to a floating work-area window.
8. **Peek slot + hide (09-08 “flowing”).** Host must (a) show the driven HWND only for look/click/type/drop, (b) fit it to the **cosmos view rect**, never the Agent column, (c) hide/park after the call or `run-ui`, (d) keep pid/id bindable while parked. Until this ships, every `look-at-window` leaves the messenger over the Agent and the user cannot monitor or work.

Do not special-case WhatsApp in the tool API (locked). The playbook above is agent-side and generic: **inner client, last-look space, printed grid, composer then drop, click send when names are only in the JPEG, peek over cosmos only, hide between ops.**
