/**
 * Local Cursor agent sidecar for StarDust UI.
 * Bind 127.0.0.1:PORT (default 6007). Rust proxies /api/agent/* here with
 * the prefix stripped (/api/agent/health → /health).
 *
 * CURSOR_API_KEY is read from the environment only — never from a file in-repo.
 * STARDUST_AGENT_CWD is the local agent workspace (temp junction folder when
 * spawned by fs-viz; otherwise process.cwd()).
 */
import http from "node:http";
import fs from "node:fs";
import { randomUUID } from "node:crypto";
import { fileURLToPath } from "node:url";
import { Agent, CursorAgentError } from "@cursor/sdk";

const HOST = "127.0.0.1";
const PORT = Number.parseInt(process.env.PORT || "6007", 10) || 6007;
const CORS_ORIGIN = "http://127.0.0.1:6006";
const HOST_UI = process.env.STARDUST_UI_URL || "http://127.0.0.1:6006";
const DEFAULT_MODEL = "composer-2.5";
const ATTACH_INLINE_MAX = 256 * 1024;
const UI_CMD_TIMEOUT_MS = 10000;
const DRIVE_TIMEOUT_MS = 20000;
const DRIVE_RUN_TIMEOUT_MS = 45000;
const SCRIPT_PATH = fileURLToPath(import.meta.url);
const UI_HINT =
  "You cannot see the StarDust cosmos or folder view until you call look-at-view. That tool returns a screenshot of the focused cosmos pane only (not menus, not the Agent panel). Drive the live StarDust cosmos with navigate, open, select, pin, and copy. To see or operate other OS apps (browser, messenger, editor), use list-windows, target-window, look-at-window / look-at-workarea / look-at-desktop, window-click, window-type, drop-file, and run-ui. Those tools never move the user's real mouse; they drive a chosen window. Prefer one run-ui call for a short sequence instead of one click per turn. Do not use Shell for UI automation (no Add-Type, mouse_event, SendKeys, CopyFromScreen, HWND_TOPMOST, or screenshot files on Desktop). Never write under AppData or stardust-ws-*. Do not assume which folder is on screen from this message. The agent working directory is a temporary junction overlay (stardust-ws-* under the OS temp folder) of split roots — it is not the user's Desktop or the focused cosmos folder. Do not Write, Shell-copy, or save onto that cwd floor. When the user says \"copy this file\", call the copy tool with absolute host paths from look-at-view or @-attachments (not SDK Write). If dest is omitted, duplicate beside the source on the real host folder. If the user is clearly on Desktop with the file attached or selected, omit dest so it duplicates on Desktop. If the destination folder is unclear, ask. Never copy into stardust-ws-*.";

function scriptMtimeSecs() {
  try {
    return Math.trunc(fs.statSync(SCRIPT_PATH).mtimeMs / 1000);
  } catch {
    return 0;
  }
}
let defaultCwd = agentCwdInitial();
/** @type {Array<{ name?: string, real?: string }>} */
let workspaceMap = [];

function agentCwdInitial() {
  return process.env.STARDUST_AGENT_CWD || process.cwd();
}

/** @typedef {{
 *   id: string,
 *   title: string,
 *   model: string,
 *   mode: "agent" | "plan" | "ask",
 *   agent: import("@cursor/sdk").SDKAgent | null,
 *   agentId: string | null,
 *   activeRun: import("@cursor/sdk").Run | null,
 *   pendingMessages: Array<{ role?: string, text?: string }>,
 *   needsContext: boolean,
 * }} Thread */

/** @type {Map<string, Thread>} */
const threads = new Map();

function hasApiKey() {
  return Boolean(process.env.CURSOR_API_KEY?.trim());
}

function apiKey() {
  return process.env.CURSOR_API_KEY?.trim() || "";
}

function agentCwd() {
  return defaultCwd;
}

function isCursorAgentError(err) {
  if (!err || typeof err !== "object") return false;
  if (typeof CursorAgentError === "function" && err instanceof CursorAgentError) {
    return true;
  }
  return err instanceof Error && typeof err.isRetryable === "boolean";
}

function cors(res) {
  res.setHeader("Access-Control-Allow-Origin", CORS_ORIGIN);
  res.setHeader("Access-Control-Allow-Methods", "GET, POST, PATCH, DELETE, OPTIONS");
  res.setHeader("Access-Control-Allow-Headers", "Content-Type");
}

function json(res, status, body) {
  cors(res);
  res.writeHead(status, {
    "Content-Type": "application/json; charset=utf-8",
    "Cache-Control": "no-store",
  });
  res.end(JSON.stringify(body));
}

async function readJson(req) {
  const chunks = [];
  for await (const chunk of req) chunks.push(chunk);
  const raw = Buffer.concat(chunks).toString("utf8").trim();
  if (!raw) return {};
  return JSON.parse(raw);
}

/** Strip /api/agent so this works if the Rust proxy has not stripped yet. */
function routePath(pathname) {
  if (pathname === "/api/agent") return "/";
  if (pathname.startsWith("/api/agent/")) {
    return pathname.slice("/api/agent".length) || "/";
  }
  return pathname;
}

function slash(p) {
  return String(p || "").replace(/\\/g, "/");
}

function hostToWorkspace(hostPath) {
  if (!hostPath || !workspaceMap.length) return hostPath;
  const lower = slash(hostPath).toLowerCase();
  for (const m of workspaceMap) {
    const real = slash(m.real || "").replace(/\/+$/, "");
    if (!real || !m.name) continue;
    const realLower = real.toLowerCase();
    if (lower === realLower) return m.name;
    if (lower.startsWith(realLower + "/")) {
      const rest = slash(hostPath).slice(real.length).replace(/^\/+/, "");
      return rest ? `${m.name}/${rest}` : m.name;
    }
  }
  return hostPath;
}

function workspaceToHost(p) {
  if (!p || !workspaceMap.length) return p;
  const n = slash(p);
  for (const m of workspaceMap) {
    const name = slash(m.name || "").replace(/\/+$/, "");
    const real = m.real || "";
    if (!name || !real) continue;
    if (n.toLowerCase() === name.toLowerCase()) return real;
    if (n.toLowerCase().startsWith(name.toLowerCase() + "/")) {
      const rest = n.slice(name.length).replace(/^\/+/, "");
      const sep = real.includes("\\") ? "\\" : "/";
      return real.replace(/[\\/]+$/, "") + sep + rest.replace(/\//g, sep);
    }
  }
  return p;
}

function overlayToHost(p) {
  if (!p) return p;
  const cwd = slash(defaultCwd).replace(/\/+$/, "");
  const n = slash(p);
  if (cwd) {
    const cl = cwd.toLowerCase();
    const nl = n.toLowerCase();
    if (nl === cl) return p;
    if (nl.startsWith(cl + "/")) {
      const rest = n.slice(cwd.length).replace(/^\/+/, "");
      if (!rest) return p;
      return workspaceToHost(rest);
    }
  }
  return workspaceToHost(p);
}

function stillTempWorkspacePath(p) {
  const s = slash(p);
  if (/(?:^|\/)stardust-ws-[^/]*/i.test(s)) return true;
  const cwd = slash(defaultCwd).replace(/\/+$/, "");
  if (!cwd) return false;
  const sl = s.toLowerCase();
  const cl = cwd.toLowerCase();
  return sl === cl || sl.startsWith(cl + "/");
}

function mapArgPaths(args) {
  if (!args || typeof args !== "object" || Array.isArray(args)) return args || {};
  const out = { ...args };
  for (const key of ["path", "dest", "to", "from"]) {
    if (typeof out[key] === "string") out[key] = overlayToHost(out[key]);
  }
  if (Array.isArray(out.paths)) {
    out.paths = out.paths.map((p) => (typeof p === "string" ? overlayToHost(p) : p));
  }
  if (Array.isArray(out.steps)) {
    out.steps = out.steps.map((s) => mapArgPaths(s));
  }
  return out;
}

async function hostJson(method, path, body, timeoutMs) {
  const ac = new AbortController();
  const timer = setTimeout(() => ac.abort(), timeoutMs);
  try {
    const res = await fetch(`${HOST_UI}${path}`, {
      method,
      headers: body !== undefined ? { "Content-Type": "application/json" } : undefined,
      body: body !== undefined ? JSON.stringify(body) : undefined,
      signal: ac.signal,
    });
    const text = await res.text();
    let data = {};
    if (text) {
      try {
        data = JSON.parse(text);
      } catch {
        data = { error: text };
      }
    }
    if (!res.ok) {
      return { ok: false, error: (data && data.error) || `host ${res.status}` };
    }
    return data && typeof data === "object" ? data : { ok: false, error: "empty host response" };
  } catch (err) {
    if (err && err.name === "AbortError") {
      return { ok: false, error: "ui command timed out" };
    }
    return { ok: false, error: err && err.message ? err.message : String(err) };
  } finally {
    clearTimeout(timer);
  }
}

async function runUiTool(op, args) {
  return hostJson("POST", "/api/ui/command", { op, args: mapArgPaths(args) }, UI_CMD_TIMEOUT_MS);
}

async function runDriveTool(op, args) {
  const ms = op === "run-ui" ? DRIVE_RUN_TIMEOUT_MS : DRIVE_TIMEOUT_MS;
  return hostJson("POST", "/api/drive", { op, args: mapArgPaths(args) }, ms);
}

function driveLookContent(data, fallback) {
  if (!data || data.ok === false) return uiFail(data, fallback);
  const lines = [];
  if (data.title) lines.push(`Title: ${data.title}`);
  if (data.width && data.height) lines.push(`Size: ${data.width}x${data.height} (bitmap coords, origin top-left)`);
  if (data.origin && (data.origin.x != null || data.origin.y != null)) {
    lines.push(`Screen origin: (${data.origin.x}, ${data.origin.y})`);
  }
  const gridPx = data.grid_px || 100;
  if (Array.isArray(data.grid) && data.grid.length) {
    const bits = data.grid
      .slice(0, 256)
      .map((c) => `${c.n}@${c.x},${c.y}`);
    lines.push(`Grid ${gridPx}px (numbered cells on the JPEG): ${bits.join(" ")}`);
  } else {
    lines.push(`Grid ${gridPx}px numbered cells on the JPEG (left-to-right, top-to-bottom).`);
  }
  if (Array.isArray(data.hits) && data.hits.length) {
    const bits = data.hits.slice(0, 80).map((h) => {
      const n = h && h.name ? h.name : "?";
      return `${n}@${h.x},${h.y} ${h.w}x${h.h}`;
    });
    lines.push(`Hits: ${bits.join("; ")}`);
  }
  if (Array.isArray(data.monitors) && data.monitors.length) {
    lines.push(
      `Monitors: ${data.monitors.map((m) => `${m.w}x${m.h}@${m.x},${m.y}${m.primary ? " primary" : ""}`).join("; ")}`
    );
  }
  if (data.error) lines.push(String(data.error));
  if (!data.image) lines.push("(No image captured.)");
  const content = [{ type: "text", text: lines.join("\n") || "ok" }];
  if (data.image) {
    const img = splitDataUrl(data.image);
    if (img.data) {
      content.push({
        type: "image",
        data: img.data,
        mimeType: img.mimeType || data.mimeType || "image/jpeg",
      });
    }
  }
  if (data.checkpoint) {
    const img = splitDataUrl(data.checkpoint);
    if (img.data) {
      content.push({
        type: "image",
        data: img.data,
        mimeType: img.mimeType || "image/jpeg",
      });
    }
  }
  return { content, isError: data.ok === false };
}

function splitDataUrl(s) {
  const raw = String(s || "");
  const m = raw.match(/^data:([^;]+);base64,(.+)$/s);
  if (m) return { mimeType: m[1], data: m[2] };
  return { mimeType: "image/jpeg", data: raw };
}

function uiFail(data, fallback) {
  return {
    content: [{ type: "text", text: (data && data.error) || fallback }],
    isError: true,
  };
}

function uiOkText(data) {
  if (!data || data.ok === false) return uiFail(data, "ui tool failed");
  const bits = [];
  if (data.path) bits.push(String(data.path));
  if (Array.isArray(data.names) && data.names.length) bits.push(data.names.join(", "));
  return { content: [{ type: "text", text: bits.length ? `ok: ${bits.join(" — ")}` : "ok" }] };
}

function uiCustomTools() {
  return {
    "look-at-view": {
      description:
        "Capture the focused StarDust cosmos/folder view. You cannot see the StarDust view until you call this. Returns a JPEG of the focused pane only (not menus, not the Agent panel), plus the folder path and star names.",
      inputSchema: { type: "object", properties: {}, additionalProperties: false },
      annotations: { title: "look-at-view", readOnlyHint: true, openWorldHint: true },
      async execute() {
        const data = await runUiTool("look-at-view", {});
        if (!data || data.ok === false) return uiFail(data, "look-at-view failed");
        const lines = [
          `Focused cosmos (${data.mode || "?"}): ${data.path || ""}`,
          Array.isArray(data.names) && data.names.length
            ? `Stars: ${data.names.join(", ")}`
            : "Stars: (none)",
          Array.isArray(data.selected) && data.selected.length
            ? `Selected: ${data.selected.join(", ")}`
            : "Selected: (none)",
        ];
        if (!data.image) lines.push("(No image captured.)");
        const content = [{ type: "text", text: lines.join("\n") }];
        if (data.image) {
          const img = splitDataUrl(data.image);
          if (img.data) {
            content.push({
              type: "image",
              data: img.data,
              mimeType: img.mimeType || data.mimeType || "image/jpeg",
            });
          }
        }
        return { content };
      },
    },
    navigate: {
      description:
        "Navigate the focused StarDust cosmos pane to a folder. Path is an absolute host path as shown by look-at-view, or 'This PC'. Call look-at-view afterward if you need to see the new folder.",
      inputSchema: {
        type: "object",
        properties: {
          path: {
            type: "string",
            description: "Absolute folder path, or This PC",
          },
        },
        required: ["path"],
        additionalProperties: false,
      },
      annotations: { title: "navigate", openWorldHint: true },
      async execute(args) {
        return uiOkText(await runUiTool("navigate", args || {}));
      },
    },
    open: {
      description:
        "Open stars in the focused StarDust cosmos (folders navigate; files open in the editor). Pass star names from look-at-view, or absolute paths.",
      inputSchema: {
        type: "object",
        properties: {
          names: {
            type: "array",
            items: { type: "string" },
            description: "Star names in the focused cosmos",
          },
          paths: {
            type: "array",
            items: { type: "string" },
            description: "Absolute star paths",
          },
        },
        additionalProperties: false,
      },
      annotations: { title: "open", openWorldHint: true },
      async execute(args) {
        return uiOkText(await runUiTool("open", args || {}));
      },
    },
    select: {
      description:
        "Select stars in the focused StarDust cosmos by name (from look-at-view). Replaces the current selection. Empty names clears the selection.",
      inputSchema: {
        type: "object",
        properties: {
          names: {
            type: "array",
            items: { type: "string" },
            description: "Star names in the focused cosmos",
          },
        },
        required: ["names"],
        additionalProperties: false,
      },
      annotations: { title: "select", openWorldHint: true },
      async execute(args) {
        return uiOkText(await runUiTool("select", args || {}));
      },
    },
    pin: {
      description:
        "Pin stars from the focused StarDust cosmos to the pin bar (toggles off if every named star is already pinned). Pass names from look-at-view, or absolute paths.",
      inputSchema: {
        type: "object",
        properties: {
          names: {
            type: "array",
            items: { type: "string" },
            description: "Star names in the focused cosmos",
          },
          paths: {
            type: "array",
            items: { type: "string" },
            description: "Absolute star paths",
          },
        },
        additionalProperties: false,
      },
      annotations: { title: "pin", openWorldHint: true },
      async execute(args) {
        return uiOkText(await runUiTool("pin", args || {}));
      },
    },
    copy: {
      description:
        "Duplicate files or folders on the real host filesystem (Desktop, etc.). Use this for \"copy this file\" / \"duplicate this\" — never SDK Write or Shell copy into the agent cwd. Paths must be absolute host paths from look-at-view, @-attachments, or select. Optional dest is a real host directory; omit dest to duplicate beside the source (same folder). If dest is missing and the current folder is not obvious, ask instead of guessing. Explorer names when the dest folder is the same as the source or the name exists: `Name - Copy.ext`, then `Name - Copy (2).ext`. Rejects destinations under stardust-ws-*.",
      inputSchema: {
        type: "object",
        properties: {
          path: {
            type: "string",
            description: "Absolute host path of one source file or folder",
          },
          paths: {
            type: "array",
            items: { type: "string" },
            description: "Absolute host paths of sources",
          },
          names: {
            type: "array",
            items: { type: "string" },
            description: "Star names in the focused cosmos (from look-at-view)",
          },
          dest: {
            type: "string",
            description:
              "Optional destination directory (absolute host path). Omit to duplicate beside the source. Never the temp overlay cwd.",
          },
        },
        additionalProperties: false,
      },
      annotations: { title: "copy", openWorldHint: true },
      async execute(args) {
        const mapped = mapArgPaths(args || {});
        const dest = mapped.dest || mapped.to;
        if (dest && stillTempWorkspacePath(dest)) {
          return uiFail(
            { error: "destination is the agent temp workspace (stardust-ws); use a real host folder such as Desktop" },
            "destination is the agent temp workspace"
          );
        }
        const data = await runUiTool("copy", args || {});
        if (!data || data.ok === false) return uiFail(data, "copy failed");
        const bits = [];
        if (Array.isArray(data.paths) && data.paths.length) bits.push(data.paths.join(", "));
        else if (Array.isArray(data.names) && data.names.length) bits.push(data.names.join(", "));
        return { content: [{ type: "text", text: bits.length ? `copied: ${bits.join(" — ")}` : "ok" }] };
      },
    },
    "list-windows": {
      description:
        "List visible top-level OS windows (title, pid, hwnd/id, outer bounds, visible client size, unusable flag for cloaked/tiny). Use before target-window. Not the StarDust cosmos.",
      inputSchema: { type: "object", properties: {}, additionalProperties: false },
      annotations: { title: "list-windows", readOnlyHint: true, openWorldHint: true },
      async execute() {
        const data = await runDriveTool("list-windows", {});
        if (!data || data.ok === false) return uiFail(data, "list-windows failed");
        const rows = Array.isArray(data.windows) ? data.windows : [];
        const text = rows
          .slice(0, 80)
          .map((w) => {
            const id = w.id || w.hwnd;
            const client =
              w.client_w != null && w.client_h != null ? ` client ${w.client_w}x${w.client_h}` : "";
            const flag = w.unusable ? " unusable" : "";
            return `${w.title} pid=${w.pid} id=${id} hwnd=${w.hwnd} ${w.w}x${w.h}@${w.x},${w.y}${client}${flag}`;
          })
          .join("\n");
        return { content: [{ type: "text", text: text || "(no windows)" }] };
      },
    },
    "target-window": {
      description:
        "Select an OS window by pid and/or id/hwnd, or by title substring. Several usable title matches without pid/id return the candidate list and error (does not pick in silence). Title-only never binds a cloaked/tiny strip. Does not leave it focused. Does not move the mouse.",
      inputSchema: {
        type: "object",
        properties: {
          title: { type: "string", description: "Case-insensitive title substring" },
          pid: { type: "integer", description: "Process id from list-windows" },
          id: { type: "string", description: "Stable hwnd id from list-windows (string or decimal)" },
          hwnd: { type: "string", description: "Alias of id" },
        },
        additionalProperties: false,
      },
      annotations: { title: "target-window", openWorldHint: true },
      async execute(args) {
        const data = await runDriveTool("target-window", args || {});
        if (!data || data.ok === false) return uiFail(data, "target-window failed");
        return { content: [{ type: "text", text: `ok: ${data.title || ""} pid=${data.pid || ""} id=${data.id || data.hwnd || ""}` }] };
      },
    },
    "look-at-window": {
      description:
        "JPEG of the target OS window (or title/pid/id), with pixel size, numbered 100px grid on the bitmap, and UIA/a11y hit-list (visible name + box in the same bitmap space as window-click). Origin top-left. Not cosmos look-at-view. Never writes a screenshot file.",
      inputSchema: {
        type: "object",
        properties: {
          title: { type: "string", description: "Optional title substring; default is current target-window" },
          pid: { type: "integer", description: "Optional process id" },
          id: { type: "string", description: "Optional hwnd id" },
        },
        additionalProperties: false,
      },
      annotations: { title: "look-at-window", readOnlyHint: true, openWorldHint: true },
      async execute(args) {
        return driveLookContent(await runDriveTool("look-at-window", args || {}), "look-at-window failed");
      },
    },
    "look-at-workarea": {
      description: "JPEG of the primary work area (desktop cover plus overlapping apps as they appear). Grid and size included.",
      inputSchema: { type: "object", properties: {}, additionalProperties: false },
      annotations: { title: "look-at-workarea", readOnlyHint: true, openWorldHint: true },
      async execute() {
        return driveLookContent(await runDriveTool("look-at-workarea", {}), "look-at-workarea failed");
      },
    },
    "look-at-desktop": {
      description: "JPEG of the full virtual desktop (every monitor). Includes each display rectangle.",
      inputSchema: { type: "object", properties: {}, additionalProperties: false },
      annotations: { title: "look-at-desktop", readOnlyHint: true, openWorldHint: true },
      async execute() {
        return driveLookContent(await runDriveTool("look-at-desktop", {}), "look-at-desktop failed");
      },
    },
    "window-click": {
      description:
        "Click the target OS window at bitmap coordinates from the last look, or by visible-text / a11y-name substring (UIA). Coordinates are in the JPEG/grid space (scaled if the capture was shrunk to max edge 1600). A coordinate click not on a hit box that leaves the UI unchanged is a miss. Never moves the real cursor. May briefly foreground the target then restore.",
      inputSchema: {
        type: "object",
        properties: {
          x: { type: "integer", description: "Bitmap X from last look" },
          y: { type: "integer", description: "Bitmap Y from last look" },
          name: { type: "string", description: "Visible text / UIA a11y-name substring from the hit-list" },
        },
        additionalProperties: false,
      },
      annotations: { title: "window-click", openWorldHint: true },
      async execute(args) {
        const data = await runDriveTool("window-click", args || {});
        if (!data || data.ok === false) return uiFail(data, "window-click failed");
        return { content: [{ type: "text", text: `ok: ${data.x},${data.y}` }] };
      },
    },
    "window-type": {
      description:
        "Type text or a chord into the target OS window. Chords: Tab, Shift+Tab, Backspace, Delete, Enter, Escape, Ctrl+A, Ctrl+L, Ctrl+V, Ctrl+F, Ctrl+Enter, Ctrl+letter. Never moves the mouse. May briefly foreground the target then restore. With contact/header/expect set, refuses typing/Enter unless the last look title or a hit name matches, unless force is true.",
      inputSchema: {
        type: "object",
        properties: {
          text: { type: "string", description: "Characters to type" },
          chord: {
            type: "string",
            description:
              "Tab, Shift+Tab, Backspace, Delete, Enter, Escape, Ctrl+A, Ctrl+L, Ctrl+V, Ctrl+F, Ctrl+Enter, or Ctrl+letter",
          },
          contact: { type: "string", description: "If set, last look title or a hit name must contain this (unless force)" },
          header: { type: "string", description: "Alias of contact" },
          expect: { type: "string", description: "Alias of contact" },
          force: { type: "boolean", description: "Skip the composer/header match guard" },
        },
        additionalProperties: false,
      },
      annotations: { title: "window-type", openWorldHint: true },
      async execute(args) {
        const data = await runDriveTool("window-type", args || {});
        if (!data || data.ok === false) return uiFail(data, "window-type failed");
        return { content: [{ type: "text", text: "ok" }] };
      },
    },
    "drop-file": {
      description:
        "Drop an absolute host file path onto the target OS window (CF_HDROP / attach). Generic, not app-specific. Rejects AppData and stardust-ws-*. Never moves the real cursor.",
      inputSchema: {
        type: "object",
        properties: {
          path: { type: "string", description: "Absolute host path of the file to attach" },
        },
        required: ["path"],
        additionalProperties: false,
      },
      annotations: { title: "drop-file", openWorldHint: true },
      async execute(args) {
        const mapped = mapArgPaths(args || {});
        if (mapped.path && stillTempWorkspacePath(mapped.path)) {
          return uiFail({ error: "path is under stardust-ws-*" }, "drop-file rejected");
        }
        const data = await runDriveTool("drop-file", args || {});
        if (!data || data.ok === false) return uiFail(data, "drop-file failed");
        return { content: [{ type: "text", text: `ok: ${data.path || mapped.path || ""}` }] };
      },
    },
    "run-ui": {
      description:
        "Run up to 20 desktop-drive steps in one host call (target-window, look-*, window-click, window-type, drop-file, list-windows). Stops on miss, drop failure, new dialog, or composer-guard failure. Returns the final JPEG, or the failure JPEG. Insert look-* only when the next click would be a guess. Do not one-click-per-turn. Optional contact/header/expect: refuse window-type / Enter unless the last look title or a hit name contains that string, unless force is true. If those fields are omitted, generic apps are not blocked.",
      inputSchema: {
        type: "object",
        properties: {
          steps: {
            type: "array",
            description: "Ordered steps; each has op plus that tool's fields",
            items: { type: "object", additionalProperties: true },
          },
          contact: { type: "string", description: "Intended messenger thread/header; required match before type/Enter unless force" },
          header: { type: "string", description: "Alias of contact" },
          expect: { type: "string", description: "Alias of contact" },
          force: { type: "boolean", description: "Skip the composer/header match guard" },
        },
        required: ["steps"],
        additionalProperties: false,
      },
      annotations: { title: "run-ui", openWorldHint: true },
      async execute(args) {
        return driveLookContent(await runDriveTool("run-ui", args || {}), "run-ui failed");
      },
    },
  };
}

function attachmentPath(item) {
  if (typeof item === "string") return item;
  if (item && typeof item.path === "string") return item.path;
  return "";
}

function fillAttachments(attachments) {
  if (!Array.isArray(attachments)) return [];
  return attachments.map((item) => {
    const hostPath = attachmentPath(item);
    const rec =
      item && typeof item === "object"
        ? { ...item, path: hostPath }
        : { path: hostPath };
    if (!hostPath) return rec;
    rec.workspacePath = hostToWorkspace(hostPath);
    if (rec.content != null && rec.content !== "") return rec;
    try {
      const st = fs.statSync(hostPath);
      if (!st.isFile() || st.size > ATTACH_INLINE_MAX) return rec;
      const buf = fs.readFileSync(hostPath);
      if (buf.includes(0)) return rec;
      rec.content = buf.toString("utf8");
    } catch {
      // path-only; agent may still open a workspace-relative path
    }
    return rec;
  });
}

function buildPrompt(prompt, attachments) {
  const user = typeof prompt === "string" ? prompt : "";
  const items = fillAttachments(attachments);
  if (!items.length) return user;
  let prefix = "Attached paths:\n";
  for (const item of items) {
    const hostPath = attachmentPath(item);
    if (!hostPath) continue;
    const wsPath = item.workspacePath || hostToWorkspace(hostPath);
    prefix += wsPath && wsPath !== hostPath ? `- ${wsPath} (${hostPath})\n` : `- ${hostPath}\n`;
    if (item && item.content != null && item.content !== "") {
      prefix += `${item.content}\n`;
    }
  }
  return user ? `${prefix}\n${user}` : prefix.trimEnd();
}

function displayToolName(event) {
  const name = (event && event.name) || "tool";
  const args = event && event.args;
  if (!args || typeof args !== "object") return name;
  const inner = args.toolName || args.tool_name;
  if (typeof inner !== "string" || !inner) return name;
  if (name === "mcp" || name === "CallMcpTool" || name === "call_mcp_tool" || /mcp/i.test(String(name))) {
    return inner;
  }
  return name;
}

function toolDetail(args) {
  if (!args || typeof args !== "object") return "";
  if (Array.isArray(args.names) && args.names.length) {
    const joined = args.names.map(String).join(", ");
    return joined.length > 80 ? `${joined.slice(0, 77)}…` : joined;
  }
  const raw =
    args.path ||
    args.file_path ||
    args.target_file ||
    args.command ||
    args.query ||
    args.pattern ||
    "";
  const detail = typeof raw === "string" ? raw : "";
  return detail.length > 80 ? `${detail.slice(0, 77)}…` : detail;
}

function toolLabel(event) {
  const name = displayToolName(event);
  const detail = toolDetail(event && event.args);
  const label = detail ? `${name} ${detail}` : name;
  const status = (event && event.status) || "running";
  if (status === "error") return `${label} (error)`;
  if (status === "completed") return label;
  return `${label}…`;
}

function publicStreamEvent(event, text) {
  const type = event && event.type;
  if (type === "thinking") {
    return { type: "thinking", text: "Thinking…" };
  }
  if (type === "tool_call") {
    return {
      type: "tool_call",
      name: displayToolName(event),
      status: event.status || "running",
      call_id: event.call_id || "",
      args: event.args,
      text: toolLabel(event),
    };
  }
  if (type === "status") {
    const status = event.status || "";
    if (status === "FINISHED" || status === "CANCELLED" || status === "ERROR" || status === "EXPIRED") {
      return null;
    }
    return {
      type: "status",
      status,
      text: "Thinking…",
    };
  }
  if (type === "task") {
    return { type: "status", text: event.text || event.status || "Working…" };
  }
  if (type === "assistant") {
    return { type: "assistant", text };
  }
  return null;
}

function resolveModel(bodyModel, threadModel) {
  if (typeof bodyModel === "string" && bodyModel.trim()) return bodyModel.trim();
  if (bodyModel && typeof bodyModel === "object" && typeof bodyModel.id === "string") {
    return bodyModel.id.trim() || DEFAULT_MODEL;
  }
  if (typeof threadModel === "string" && threadModel.trim()) return threadModel.trim();
  return DEFAULT_MODEL;
}

function assistantTextFromEvent(event) {
  if (!event || event.type !== "assistant") return "";
  const parts = event.message?.content;
  if (!Array.isArray(parts)) return "";
  let text = "";
  for (const block of parts) {
    if (block && block.type === "text" && typeof block.text === "string") {
      text += block.text;
    }
  }
  return text;
}

async function disposeAgent(agent) {
  if (!agent) return;
  try {
    if (typeof agent[Symbol.asyncDispose] === "function") {
      await agent[Symbol.asyncDispose]();
    } else if (typeof agent.close === "function") {
      await agent.close();
    }
  } catch {
    // best-effort
  }
}

function resolveUiMode(raw) {
  if (raw === "plan" || raw === "ask") return raw;
  return "agent";
}

function sdkModeOf(uiMode) {
  return uiMode === "plan" ? "plan" : "agent";
}

function askDisallowed(uiMode) {
  return uiMode === "ask" ? ["edit", "delete", "applyAgentDiff"] : null;
}

function publicThread(thread) {
  return {
    id: thread.id,
    title: thread.title,
    model: thread.model,
    mode: thread.mode || "agent",
    agentId: thread.agentId || null,
  };
}

function conversationPrefix(messages) {
  if (!Array.isArray(messages) || messages.length === 0) return "";
  let out = "Prior conversation (resume after restart):\n";
  for (const m of messages) {
    const role =
      m && m.role === "user" ? "User" : m && m.role === "assistant" ? "Assistant" : "Note";
    const text = m && typeof m.text === "string" ? m.text.trim() : "";
    if (!text) continue;
    out += `${role}: ${text}\n`;
  }
  return out ? `${out}\n` : "";
}

function agentCreateOpts(thread, model) {
  const uiMode = resolveUiMode(thread.mode);
  const opts = {
    apiKey: apiKey(),
    model: { id: model },
    local: { cwd: agentCwd(), customTools: uiCustomTools() },
    name: thread.title || undefined,
    mode: sdkModeOf(uiMode),
  };
  const disallowed = askDisallowed(uiMode);
  if (disallowed) opts.disallowedTools = disallowed;
  return opts;
}

async function ensureAgent(thread, model) {
  if (thread.agent) return thread.agent;
  const opts = agentCreateOpts(thread, model);
  if (thread.agentId && typeof Agent.resume === "function") {
    try {
      const resumed = await Agent.resume(thread.agentId, opts);
      thread.agent = resumed;
      thread.model = model;
      thread.agentId = resumed.agentId || thread.agentId;
      thread.needsContext = false;
      thread.pendingMessages = [];
      return resumed;
    } catch {
      // fall through to create + prepend saved turns
    }
  }
  const agent = await Agent.create(opts);
  thread.agent = agent;
  thread.model = model;
  thread.agentId = agent.agentId || thread.agentId;
  return agent;
}

function mergeAssistantText(prev, next, newSegment) {
  if (!next) return prev;
  if (!prev) return next;
  if (next === prev) return prev;
  if (next.startsWith(prev)) return next;
  if (newSegment) return next;
  if (prev.startsWith(next)) return prev;
  return prev + next;
}

async function runPrompt(thread, body, onEvent) {
  if (!hasApiKey()) {
    const err = new Error("CURSOR_API_KEY missing");
    err.code = "missing_key";
    throw err;
  }
  const model = resolveModel(body.model, thread.model);
  const uiMode = resolveUiMode(body.mode || thread.mode);
  if (thread.agent && (thread.model !== model || resolveUiMode(thread.mode) !== uiMode)) {
    await disposeAgent(thread.agent);
    thread.agent = null;
  }
  thread.model = model;
  thread.mode = uiMode;
  const agent = await ensureAgent(thread, model);
  let fullPrompt = buildPrompt(body.prompt, body.attachments);
  if (thread.needsContext) {
    const prefix = conversationPrefix(thread.pendingMessages);
    if (prefix) fullPrompt = prefix + fullPrompt;
    thread.needsContext = false;
    thread.pendingMessages = [];
  }
  const rules = typeof body.rules === "string" ? body.rules.trim() : "";
  if (rules) {
    fullPrompt = `${rules}\n\n${fullPrompt}`;
  }
  if (!thread.uiHinted) {
    fullPrompt = `${UI_HINT}\n\n${fullPrompt}`;
    thread.uiHinted = true;
  }
  const sendOpts = {
    model: { id: model },
    mode: sdkModeOf(uiMode),
    local: { customTools: uiCustomTools() },
  };
  const disallowed = askDisallowed(uiMode);
  if (disallowed) sendOpts.disallowedTools = disallowed;
  const run = await agent.send(fullPrompt, sendOpts);
  thread.activeRun = run;
  let text = "";
  let committed = "";
  let afterTool = false;
  try {
    const canStream = typeof run.supports !== "function" || run.supports("stream");
    if (canStream) {
      for await (const event of run.stream()) {
        if (event && event.type === "tool_call") afterTool = true;
        const piece = assistantTextFromEvent(event);
        if (piece) {
          if (afterTool && text && !piece.startsWith(text)) {
            committed += text;
            text = piece;
          } else {
            text = mergeAssistantText(text, piece, afterTool);
          }
          afterTool = false;
        }
        if (onEvent) onEvent(event, committed + text);
      }
    } else if (typeof run.supports !== "function" || run.supports("wait")) {
      const result = await run.wait();
      if (typeof result.result === "string" && result.result) text = result.result;
      if (result.status === "error" && result.error && result.error.message) {
        throw new Error(result.error.message);
      }
    }
    return { text: committed + text, status: "finished" };
  } finally {
    thread.activeRun = null;
  }
}

function isUntitled(title) {
  const t = String(title || "").trim();
  return !t || t === "New thread";
}

async function nameUntitledThread(thread, userText) {
  if (!hasApiKey() || !isUntitled(thread.title)) return;
  const src = String(userText || "").trim();
  if (!src) return;
  try {
    const result = await Agent.prompt(
      "Reply with only a short chat title (2-6 words). No quotes, no trailing punctuation, no filename, no @mention. Name the user's goal.\n\n" +
        src.slice(0, 800),
      {
        apiKey: apiKey(),
        model: { id: thread.model || DEFAULT_MODEL },
        tools: [],
        local: { cwd: agentCwd() },
      }
    );
    let raw = "";
    if (result && typeof result.result === "string") raw = result.result;
    const title = String(raw)
      .trim()
      .split(/\r?\n/)[0]
      .replace(/^["'`“”]+|["'“”'`]+$/g, "")
      .replace(/[.!?]+$/g, "")
      .trim()
      .slice(0, 48)
      .trim();
    if (title) thread.title = title;
  } catch (_) {
    /* leave New thread */
  }
}

async function handleSend(thread, body, res) {
  try {
    const out = await runPrompt(thread, body, null);
    await nameUntitledThread(thread, body && body.prompt);
    json(res, 200, {
      ok: true,
      text: out.text,
      status: out.status,
      agentId: thread.agentId || null,
      title: thread.title || null,
    });
  } catch (err) {
    thread.activeRun = null;
    if (err && err.code === "missing_key") {
      json(res, 400, { error: "CURSOR_API_KEY missing" });
      return;
    }
    if (isCursorAgentError(err)) {
      json(res, 200, { ok: false, error: err.message || String(err) });
      return;
    }
    json(res, 500, { ok: false, error: err.message || String(err) });
  }
}

function sseWrite(res, payload) {
  if (!payload) return;
  res.write(`data: ${JSON.stringify(payload)}\n\n`);
  if (typeof res.flush === "function") res.flush();
}

async function handleStream(thread, body, res) {
  cors(res);
  res.writeHead(200, {
    "Content-Type": "text/event-stream; charset=utf-8",
    "Cache-Control": "no-store",
    Connection: "keep-alive",
    "X-Accel-Buffering": "no",
  });
  if (res.socket && typeof res.socket.setNoDelay === "function") {
    res.socket.setNoDelay(true);
  }
  sseWrite(res, { type: "status", text: "Thinking…" });
  try {
    const out = await runPrompt(thread, body, (event, text) => {
      const payload = publicStreamEvent(event, text);
      if (payload) sseWrite(res, payload);
    });
    await nameUntitledThread(thread, body && body.prompt);
    sseWrite(res, {
      type: "done",
      text: out.text,
      status: out.status,
      agentId: thread.agentId || null,
      title: thread.title || null,
    });
  } catch (err) {
    thread.activeRun = null;
    const msg =
      err && err.code === "missing_key"
        ? "CURSOR_API_KEY missing"
        : err && err.message
          ? err.message
          : String(err);
    sseWrite(res, { type: "run_error", error: msg });
  }
  res.end();
}

async function handleDelete(threadId, res) {
  const thread = threads.get(threadId);
  if (!thread) {
    json(res, 404, { error: "thread not found" });
    return;
  }
  threads.delete(threadId);
  await disposeAgent(thread.agent);
  json(res, 200, { ok: true });
}

async function handleStop(thread, res) {
  const run = thread.activeRun;
  if (run && typeof run.cancel === "function") {
    try {
      if (typeof run.supports !== "function" || run.supports("cancel")) {
        await run.cancel();
      }
    } catch {
      // cancel is best-effort
    }
  }
  json(res, 200, { ok: true });
}

async function handle(req, res) {
  const url = new URL(req.url || "/", `http://${HOST}`);
  const pathname = routePath(url.pathname);
  const method = req.method || "GET";

  if (method === "OPTIONS") {
    cors(res);
    res.writeHead(204);
    res.end();
    return;
  }

  try {
    if (method === "POST" && pathname === "/workspace") {
      const body = await readJson(req);
      if (typeof body.cwd !== "string" || !body.cwd.trim()) {
        json(res, 400, { ok: false, error: "cwd is required" });
        return;
      }
      const next = body.cwd.trim();
      if (Array.isArray(body.map)) workspaceMap = body.map;
      if (next !== defaultCwd) {
        defaultCwd = next;
        for (const t of threads.values()) {
          await disposeAgent(t.agent);
          t.agent = null;
        }
      }
      json(res, 200, { ok: true, cwd: defaultCwd });
      return;
    }

    if (method === "GET" && pathname === "/health") {
      const scriptMtime = scriptMtimeSecs();
      if (!hasApiKey()) {
        json(res, 200, { ok: false, error: "CURSOR_API_KEY missing", scriptMtime });
      } else {
        json(res, 200, { ok: true, hasKey: true, scriptMtime });
      }
      return;
    }

    if (method === "GET" && pathname === "/threads") {
      json(res, 200, {
        threads: [...threads.values()].map(publicThread),
      });
      return;
    }

    if (method === "POST" && pathname === "/threads") {
      const body = await readJson(req);
      const model = resolveModel(body.model, "");
      const requested = typeof body.id === "string" ? body.id.trim() : "";
      const existing = requested ? threads.get(requested) : null;
      if (existing) {
        if (typeof body.title === "string" && body.title.trim()) {
          existing.title = body.title.trim();
        }
        if (body.mode != null) existing.mode = resolveUiMode(body.mode);
        json(res, 200, publicThread(existing));
        return;
      }
      const id =
        requested && !requested.startsWith("local-") ? requested : randomUUID();
      const title =
        typeof body.title === "string" && body.title.trim()
          ? body.title.trim()
          : "New thread";
      const agentId =
        typeof body.agentId === "string" && body.agentId.trim()
          ? body.agentId.trim()
          : null;
      const pendingMessages = Array.isArray(body.messages) ? body.messages : [];
      threads.set(id, {
        id,
        title,
        model,
        mode: resolveUiMode(body.mode),
        agent: null,
        agentId,
        activeRun: null,
        pendingMessages,
        needsContext: pendingMessages.length > 0,
      });
      json(res, 200, publicThread(threads.get(id)));
      return;
    }

    const patchMatch = pathname.match(/^\/threads\/([^/]+)$/);
    if (method === "PATCH" && patchMatch) {
      const thread = threads.get(decodeURIComponent(patchMatch[1]));
      if (!thread) {
        json(res, 404, { error: "thread not found" });
        return;
      }
      const body = await readJson(req);
      if (typeof body.title === "string" && body.title.trim()) {
        thread.title = body.title.trim();
      }
      json(res, 200, publicThread(thread));
      return;
    }

    const sendMatch = pathname.match(/^\/threads\/([^/]+)\/send$/);
    if (method === "POST" && sendMatch) {
      const thread = threads.get(decodeURIComponent(sendMatch[1]));
      if (!thread) {
        json(res, 404, { error: "thread not found" });
        return;
      }
      await handleSend(thread, await readJson(req), res);
      return;
    }

    const streamMatch = pathname.match(/^\/threads\/([^/]+)\/stream$/);
    if (method === "POST" && streamMatch) {
      const thread = threads.get(decodeURIComponent(streamMatch[1]));
      if (!thread) {
        json(res, 404, { error: "thread not found" });
        return;
      }
      await handleStream(thread, await readJson(req), res);
      return;
    }

    const delMatch = pathname.match(/^\/threads\/([^/]+)$/);
    if (method === "DELETE" && delMatch) {
      await handleDelete(decodeURIComponent(delMatch[1]), res);
      return;
    }

    const stopMatch = pathname.match(/^\/threads\/([^/]+)\/stop$/);
    if (method === "POST" && stopMatch) {
      const thread = threads.get(decodeURIComponent(stopMatch[1]));
      if (!thread) {
        json(res, 404, { error: "thread not found" });
        return;
      }
      await handleStop(thread, res);
      return;
    }

    json(res, 404, { error: "not found" });
  } catch (err) {
    if (err instanceof SyntaxError) {
      json(res, 400, { error: "invalid JSON" });
      return;
    }
    if (isCursorAgentError(err)) {
      json(res, 200, { ok: false, error: err.message || String(err) });
      return;
    }
    json(res, 500, { error: err.message || String(err) });
  }
}

const server = http.createServer((req, res) => {
  void handle(req, res);
});

server.listen(PORT, HOST, () => {
  console.log(`agent sidecar http://${HOST}:${PORT}`);
});

async function shutdown() {
  server.close();
  const agents = [...threads.values()].map((t) => t.agent);
  threads.clear();
  await Promise.all(agents.map(disposeAgent));
  process.exit(0);
}

process.on("SIGINT", () => {
  void shutdown();
});
process.on("SIGTERM", () => {
  void shutdown();
});
