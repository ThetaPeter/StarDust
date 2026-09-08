(function () {
  const $ = (s, r) => (r || document).querySelector(s);
  const $$ = (s, r) => [...(r || document).querySelectorAll(s)];

  function sepOf(p) {
    return p && p.indexOf("\\") >= 0 && p.indexOf("/") < 0 ? "\\" : "/";
  }
  function joinPath(parent, name) {
    if (!parent) return name;
    if (!name) return parent;
    const s = sepOf(parent);
    return parent.endsWith("/") || parent.endsWith("\\") ? parent + name : parent + s + name;
  }
  function baseName(p) {
    if (!p) return "";
    const parts = String(p).replace(/\\/g, "/").split("/").filter(Boolean);
    return parts[parts.length - 1] || p;
  }
  function parentPath(p) {
    if (!p) return "";
    const s = sepOf(p);
    const norm = String(p).replace(/[\\/]+$/, "");
    const i = Math.max(norm.lastIndexOf("/"), norm.lastIndexOf("\\"));
    if (i <= 0) return s === "\\" && /^[A-Za-z]:$/.test(norm) ? norm + "\\" : "";
    if (s === "\\" && i === 2 && norm[1] === ":") return norm.slice(0, 3);
    return norm.slice(0, i);
  }
  function pathParts(p) {
    if (!p) return [];
    const raw = String(p);
    if (raw === ":ThisPC") return [{ name: "This PC", path: ":ThisPC" }];
    const s = sepOf(raw);
    if (s === "\\") {
      const m = raw.match(/^([A-Za-z]:)[\\/]?(.*)$/);
      if (m) {
        const rest = m[2] ? m[2].split(/[\\/]/).filter(Boolean) : [];
        return [{ name: "This PC", path: ":ThisPC" }, { name: m[1] + "\\", path: m[1] + "\\" }].concat(
          rest.map((_, i) => ({
            name: rest[i],
            path: m[1] + "\\" + rest.slice(0, i + 1).join("\\"),
          }))
        );
      }
    }
    const bits = raw.split("/").filter(Boolean);
    const acc = [];
    const rooted = raw.startsWith("/");
    bits.forEach((b, i) => {
      const path = (rooted ? "/" : "") + bits.slice(0, i + 1).join("/");
      acc.push({ name: b, path });
    });
    return acc;
  }

  class ApiError extends Error {
    constructor(status, body, url) {
      super((body && (body.error || body.message)) || "HTTP " + status + " " + url);
      this.status = status;
      this.body = body;
      this.url = url;
    }
  }

  async function apiJSON(method, url, body, timeoutMs) {
    const opts = { method, headers: {} };
    if (body !== undefined) {
      opts.headers["Content-Type"] = "application/json";
      opts.body = JSON.stringify(body);
    }
    let timer = 0;
    if (timeoutMs > 0 && typeof AbortController !== "undefined") {
      const ac = new AbortController();
      opts.signal = ac.signal;
      timer = setTimeout(function () {
        ac.abort();
      }, timeoutMs);
    }
    let res;
    try {
      res = await fetch(url, opts);
    } catch (e) {
      const aborted = e && (e.name === "AbortError" || e.code === 20);
      throw new ApiError(0, { error: aborted ? "request timed out" : String(e) }, url);
    } finally {
      if (timer) clearTimeout(timer);
    }
    const text = await res.text();
    let data = null;
    if (text) {
      try {
        data = JSON.parse(text);
      } catch (_) {
        data = { raw: text };
      }
    }
    if (!res.ok) throw new ApiError(res.status, data, url);
    return data;
  }

  function errText(e) {
    if (!e) return "error";
    if (e instanceof ApiError) {
      if (e.status === 404) return "not available yet (" + e.url + ")";
      if (e.status === 503) return "unavailable (503)";
      return e.message;
    }
    return String(e.message || e);
  }

  function normalizeChildren(data, dirPath) {
    const list = Array.isArray(data)
      ? data
      : (data && (data.entries || data.children || data.nodes)) || [];
    return list.map((e) => {
      const kind = e.kind === "dir" || e.kind === "directory" || e.isDir ? "dir" : "file";
      return {
        name: e.name || e.fileName || baseName(e.path),
        path: e.path || joinPath(dirPath, e.name || e.fileName),
        kind,
        size: e.size || 0,
        mtime: e.mtime,
        isText: kind === "dir" ? undefined : e.isText,
      };
    });
  }

  function unwrapLayout(data) {
    if (!data || typeof data !== "object") return {};
    if (data.layout && typeof data.layout === "object") return data.layout;
    return data;
  }

  const state = {
    meta: { defaultRoot: "", layoutPath: "", hasCursorKey: false },
    mode: "2d",
    agentOpen: true,
    agentWidth: 380,
    agentModel: "composer-2.5",
    sortMode: "galaxy",
    views: [],
    split: null,
    focusedViewId: null,
    focusHist: [],
    pins: [],
    starPos: { "2d": {}, "3d": {} },
    starLooks: {},
    threads: [],
    fileClip: null,
    lastWorld: { x: 0, y: 0 },
    lastClient: { x: 0, y: 0 },
    lastViewId: null,
    saveTimer: null,
    watchers: new Map(),
    vid: 1,
    threeFailed: false,
    lastFocus: "view",
    ephemeral: false,
    uiMode: "windows",
  };

  const THIS_PC = ":ThisPC";

  const api = {
    get: (u) => apiJSON("GET", u),
    post: (u, b) => apiJSON("POST", u, b),
    put: (u, b) => apiJSON("PUT", u, b),
    patch: (u, b) => apiJSON("PATCH", u, b),
    delete: (u) => apiJSON("DELETE", u),
  };

  function toast(msg) {
    let wrap = $(".toast-wrap");
    if (!wrap) {
      wrap = document.createElement("div");
      wrap.className = "toast-wrap";
      document.body.appendChild(wrap);
    }
    const t = document.createElement("div");
    t.className = "toast";
    t.textContent = msg;
    wrap.appendChild(t);
    setTimeout(() => t.remove(), 4200);
  }

  function banner(msg) {
    const b = $("#banner");
    if (!msg) {
      b.hidden = true;
      return;
    }
    b.hidden = false;
    b.textContent = msg;
  }

  function scheduleSave() {
    clearTimeout(state.saveTimer);
    state.saveTimer = setTimeout(saveLayout, 500);
  }

  function sessionRoot() {
    return state.meta.sessionRoot || state.meta.desktopPath || state.meta.defaultRoot || "";
  }

  function applyShellClass() {
    const q = new URLSearchParams(location.search || "");
    const desk = q.get("desktop") === "1" || !!q.get("open") || state.uiMode === "stardust";
    document.documentElement.classList.toggle("shell-desktop", desk);
  }

  function collectLayout() {
    return {
      mode: viewMode(focusedView()) || state.mode,
      agentOpen: state.agentOpen,
      agentWidth: state.agentWidth,
      agentModel: state.agentModel,
      sortMode: state.sortMode,
      views: state.views.map((v) => {
        if (v.cns) {
          return {
            id: v.id,
            root: v.root,
            path: parentPath(v.cns.sdPath) || v.root,
            mode: viewMode(v),
            camera: undefined,
            minimized: !!v.minimized,
          };
        }
        return {
        id: v.id,
        root: v.root,
        path: v.path,
        mode: viewMode(v),
        camera: v.cosmos ? v.cosmos.getCamera() : v.camera,
        minimized: !!v.minimized,
        };
      }),
      split: serializeSplit(state.split),
      floats: SD.editor ? SD.editor.serialize() : [],
      pins: state.pins,
      starPos: normalizeStarPos(state.starPos),
      starLooks: state.starLooks,
    };
  }

  async function saveLooksMerge() {
    const saved = unwrapLayout(await api.get("/api/layout"));
    const merged = saved && typeof saved === "object" ? Object.assign({}, saved) : {};
    merged.starLooks = state.starLooks;
    const nextPos = mergeStarPos(saved && saved.starPos, state.starPos);
    merged.starPos = nextPos;
    state.starPos = nextPos;
    await api.put("/api/layout", merged);
  }

  async function saveLayout() {
    try {
      if (state.ephemeral) await saveLooksMerge();
      else await api.put("/api/layout", collectLayout());
    } catch (_) {}
  }

  async function flushSave() {
    clearTimeout(state.saveTimer);
    state.saveTimer = 0;
    await saveLayout();
  }

  function splitAxisOf(node) {
    return node && node.axis === "y" ? "y" : "x";
  }

  function splitAxisForPane(view) {
    const el = (view && view.rootEl) || $("#splits");
    if (!el || typeof el.getBoundingClientRect !== "function") return "x";
    const r = el.getBoundingClientRect();
    return r.height > r.width ? "y" : "x";
  }

  function serializeSplit(node) {
    if (!node) return null;
    if (node.type === "leaf") return { type: "leaf", viewId: node.viewId };
    return {
      type: "split",
      axis: splitAxisOf(node),
      ratio: node.ratio,
      a: serializeSplit(node.a),
      b: serializeSplit(node.b),
    };
  }

  function viewById(id) {
    return state.views.find((v) => v.id === id);
  }
  function focusedView() {
    const v = viewById(state.focusedViewId);
    if (v && !v.minimized) return v;
    return latestUnminimized();
  }

  function latestUnminimized(except) {
    const hist = state.focusHist || [];
    for (let i = hist.length - 1; i >= 0; i--) {
      const x = viewById(hist[i]);
      if (x && !x.minimized && x !== except) return x;
    }
    return state.views.find((x) => x && !x.minimized && x !== except) || null;
  }
  function cosmosOf(view) {
    return view && (viewMode(view) === "3d" ? view.cosmos3 : view.cosmos2);
  }
  function viewMode(view) {
    return view && view.mode === "3d" ? "3d" : "2d";
  }
  function posBag(view) {
    return viewMode(view) === "3d" ? state.starPos["3d"] : state.starPos["2d"];
  }

  function pathKey(p) {
    return String(p || "")
      .replace(/\//g, "\\")
      .replace(/\\+$/, "")
      .toLowerCase();
  }

  function collapsePosBag(bag) {
    if (!bag || typeof bag !== "object") return {};
    const out = {};
    Object.keys(bag).forEach((key) => {
      const k = pathKey(key);
      if (!k) return;
      const map = bag[key];
      if (!map || typeof map !== "object" || Array.isArray(map)) return;
      out[k] = Object.assign(out[k] || {}, map);
    });
    return out;
  }

  function normalizeStarPos(sp) {
    const src = sp && typeof sp === "object" ? sp : {};
    return {
      "2d": collapsePosBag(src["2d"] || src.positions2d || {}),
      "3d": collapsePosBag(src["3d"] || src.positions3d || {}),
    };
  }

  function mergeStarPos(base, overlay) {
    const a = normalizeStarPos(base);
    const b = normalizeStarPos(overlay);
    return {
      "2d": Object.assign({}, a["2d"], b["2d"]),
      "3d": Object.assign({}, a["3d"], b["3d"]),
    };
  }

  function dirPositions(bag, dirPath) {
    if (!bag) return undefined;
    const k = pathKey(dirPath);
    if (k && bag[k]) return bag[k];
    if (dirPath && bag[dirPath]) return bag[dirPath];
    if (!k) return undefined;
    const keys = Object.keys(bag);
    for (let i = 0; i < keys.length; i++) {
      if (pathKey(keys[i]) === k) return bag[keys[i]];
    }
    return undefined;
  }

  function storeDirPositions(bag, dirPath, map) {
    const k = pathKey(dirPath);
    if (!k || !bag) return;
    Object.keys(bag).forEach((key) => {
      if (key !== k && pathKey(key) === k) delete bag[key];
    });
    bag[k] = map;
  }

  function takeDirPositions(bag, dirPath) {
    const map = dirPositions(bag, dirPath);
    if (map) storeDirPositions(bag, dirPath, map);
    return map;
  }

  function focusView(view) {
    if (!view || view.minimized) return;
    state.focusedViewId = view.id;
    state.lastFocus = "view";
    state.focusHist = (state.focusHist || []).filter((id) => id !== view.id);
    state.focusHist.push(view.id);
    $$(".cview").forEach((el) => el.classList.toggle("focused", el.dataset.id === view.id));
    if (SD.editor && SD.editor.blur) SD.editor.blur();
    refreshModeButton();
    refreshEditMenu();
    paintPinStars();
    syncSearchPlaceholder();
  }

  function focusWindow(w) {
    SD.focusedWin = w;
    state.lastFocus = "win";
  }

  function refreshViewMenu() {
    const n = SD.editor && SD.editor.arrangeCount ? SD.editor.arrangeCount() : 0;
    ["tile", "cascade"].forEach((cmd) => {
      const b = $('#menu-view [data-cmd="' + cmd + '"]');
      if (!b) return;
      b.disabled = n === 0;
      b.title =
        n === 0
          ? "Arranges floating file windows — open a text file first"
          : cmd === "tile"
            ? "Tile file windows in a grid"
            : "Stack file windows with an offset";
    });
    const el = $("#btn-ui-mode");
    if (el) el.textContent = state.uiMode === "stardust" ? "StarDust UI" : "Windows UI";
  }

  function refreshEditMenu() {
    const sel = selection();
    $$("#menu-edit [data-need-sel]").forEach((b) => {
      b.disabled = !sel.length;
    });
    const paste = $('#menu-edit [data-cmd="paste"]');
    if (paste) paste.disabled = !state.fileClip;
  }

  function selection() {
    const v = focusedView();
    const c = cosmosOf(v);
    return c && c.getSelection ? c.getSelection() : (v && v.selection) || [];
  }

  function actionStars(star) {
    const sel = selection();
    if (star && sel.length > 1 && sel.some((s) => s.path === star.path || s.name === star.name)) {
      return sel;
    }
    if (star) return [star];
    return sel;
  }

  function attachStars(stars) {
    if (!SD.agent || !stars || !stars.length) return;
    SD.agent.attach(stars.map((s) => s.path).filter(Boolean));
  }

  function broadcastLooks() {
    state.views.forEach((v) => {
      if (v.cosmos2 && v.cosmos2.setLooks) v.cosmos2.setLooks(state.starLooks);
      if (v.cosmos3 && v.cosmos3.setLooks) v.cosmos3.setLooks(state.starLooks);
    });
  }

  function setStarLook(star, look) {
    if (!star || !star.path) return;
    const k = pathKey(star.path);
    if (!look) delete state.starLooks[k];
    else state.starLooks[k] = look;
    broadcastLooks();
    scheduleSave();
  }

  let starImageInp = null;

  function starImageInput() {
    if (starImageInp && starImageInp.isConnected) return starImageInp;
    const inp = document.createElement("input");
    inp.type = "file";
    inp.accept = "image/png,image/jpeg,image/webp,image/gif";
    inp.setAttribute("aria-hidden", "true");
    inp.tabIndex = -1;
    inp.style.cssText =
      "position:fixed;left:0;top:0;width:1px;height:1px;opacity:0;overflow:hidden;border:0;padding:0;margin:0;";
    document.body.appendChild(inp);
    starImageInp = inp;
    return inp;
  }

  function pickStarImage(star) {
    const inp = starImageInput();
    inp.value = "";
    inp.onchange = () => {
      const f = inp.files && inp.files[0];
      if (!f) return;
      const url = URL.createObjectURL(f);
      const img = new Image();
      img.onload = () => {
        const S = 192;
        const c = document.createElement("canvas");
        c.width = S;
        c.height = S;
        const g = c.getContext("2d");
        const sc = Math.min(S / Math.max(1, img.width), S / Math.max(1, img.height));
        const w = img.width * sc;
        const h = img.height * sc;
        g.drawImage(img, (S - w) / 2, (S - h) / 2, w, h);
        URL.revokeObjectURL(url);
        setStarLook(star, { image: c.toDataURL("image/png") });
      };
      img.onerror = () => URL.revokeObjectURL(url);
      img.src = url;
    };
    if (typeof inp.showPicker === "function") {
      try {
        inp.showPicker();
        return;
      } catch (_) {}
    }
    inp.click();
  }

  function lookMenuItems(star) {
    if (!star) return [];
    const has = !!state.starLooks[pathKey(star.path)];
    const items = [
      { label: "Set image…", fn: () => pickStarImage(star) },
      { label: "Reset image", off: !has, fn: () => setStarLook(star, null) },
    ];
    if (star.kind === "dir") {
      const kinds = (window.SDLook && window.SDLook.dirKinds) || [];
      if (kinds.length) items.push("hr");
      kinds.forEach((st) => {
        items.push({
          label: st,
          fn: () => setStarLook(star, { dirStyle: st }),
        });
      });
    }
    return [{ label: "Image", items }];
  }

  function targetStar(view) {
    const sel = selection();
    if (sel[0]) return sel[0];
    const c = cosmosOf(view);
    return c && c.getHover ? c.getHover() : null;
  }

  function hostHandlers(view) {
    return {
      onEnter(path) {
        if (view.cns) {
          const c = cosmosOf(view);
          const stars = c && c.stars ? c.stars() : [];
          const s = stars.find((x) => x.path === path || x.name === path);
          if (s) openCnsStar(view, s);
          return;
        }
        navigateView(view, path, true);
      },
      onUp() {
        cnsLeave(view);
      },
      scrollLeave() {
        return !view.cns;
      },
      onPositions(dirPath, map) {
        if (view.cns) return;
        storeDirPositions(posBag(view), dirPath, map);
        scheduleSave();
      },
      onSelect(stars) {
        view.selection = stars;
        focusView(view);
        refreshEditMenu();
        if (view.cns && stars && stars[0] && stars[0].role !== "cluster") {
          showCnsDetail(view, stars[0].name, false);
        }
      },
      onHover(star) {
        if (view.cns && star && star.role !== "cluster") showCnsDetail(view, star.name, true);
      },
      onCamera() {
        if (!view.cns) return;
        clearTimeout(view.cnsCamTimer);
        view.cnsCamTimer = setTimeout(() => loadCnsStars(view), 90);
      },
      onActivate() {
        focusView(view);
      },
      onContextMenu(e, star) {
        focusView(view);
        const x = e.clientX;
        const y = e.clientY;
        setTimeout(() => showExplorerMenu({ clientX: x, clientY: y }, view, star), 0);
      },
      onDblClick(_e, star) {
        if (star) openStar(view, star);
        else if (view.cns) cnsLeaveNeighborhood(view);
      },
      onWorldPointer(w, client) {
        state.lastWorld = w;
        state.lastClient = client;
        state.lastViewId = view.id;
      },
      onStarDragEnd(star, e) {
        if (view.cns) return;
        const over = viewAtClient(e.clientX, e.clientY);
        if (over && over.id !== view.id && star) {
          const dest = joinPath(over.path, star.name);
          const fn = e.shiftKey ? "move" : "copy";
          mutate(fn, { from: star.path, to: dest }).then(() => {
            loadChildren(view);
            loadChildren(over);
          });
        }
      },
      onDrop(e) {
        handleDrop(e, view);
      },
    };
  }

  function viewAtClient(x, y) {
    for (const v of state.views) {
      if (!v.rootEl) continue;
      const r = v.rootEl.getBoundingClientRect();
      if (x >= r.left && x <= r.right && y >= r.top && y <= r.bottom) return v;
    }
    return null;
  }

  async function ensureEngine(view) {
    if (viewMode(view) === "3d") {
      if (!view.cosmos3) {
        try {
          await Cosmos3D.ensure();
        } catch (_) {
          state.threeFailed = true;
        }
        view.cosmos3 = Cosmos3D.create(view.stage.querySelector(".vcosmos"), hostHandlers(view));
      }
      if (view.cosmos2) {
        if (view.cosmos2.canvas) view.cosmos2.canvas.style.display = "none";
        if (view.cosmos2.statusEl) view.cosmos2.statusEl.style.display = "none";
      }
      const c3 = view.stage.querySelector(".c3d");
      if (c3) c3.style.display = "block";
      const labs = view.stage.querySelector(".c3d-labels");
      if (labs) labs.style.display = "block";
      const miss = view.stage.querySelector(".c3d-missing");
      if (miss) miss.style.display = "flex";
      if (view.cosmos3 && view.cosmos3.statusEl) view.cosmos3.statusEl.style.display = "";
      if (view.cosmos2 && view.cosmos2.setLive) view.cosmos2.setLive(false);
      if (view.cosmos3 && view.cosmos3.setLive) view.cosmos3.setLive(true);
      return view.cosmos3;
    }
    if (view.cosmos3) {
      const c3 = view.stage.querySelector(".c3d");
      if (c3) c3.style.display = "none";
      const labs = view.stage.querySelector(".c3d-labels");
      if (labs) labs.style.display = "none";
      const miss = view.stage.querySelector(".c3d-missing");
      if (miss) miss.style.display = "none";
      if (view.cosmos3.statusEl) view.cosmos3.statusEl.style.display = "none";
      if (view.cosmos3.setLive) view.cosmos3.setLive(false);
    }
    if (view.cosmos2) {
      if (view.cosmos2.canvas) view.cosmos2.canvas.style.display = "block";
      if (view.cosmos2.statusEl) view.cosmos2.statusEl.style.display = "";
      if (view.cosmos2.setLive) view.cosmos2.setLive(true);
    }
    return view.cosmos2;
  }

  async function loadChildren(view, opts) {
    opts = opts || {};
    if (view.cns) {
      await loadCnsStars(view, opts);
      return;
    }
    const c0 = await ensureEngine(view);
    view.cosmos = c0;
    if (c0 && c0.setScene) c0.setScene({ dirPath: view.path, loading: true, children: [] });
    let kids = [];
    let error = "";
    try {
      if (view.path === THIS_PC) {
        const data = await api.get("/api/drives");
        kids = (data.drives || []).map((d) => ({
          name: d.name || d.path,
          path: d.path,
          kind: "dir",
          size: d.total || 0,
          free: d.free,
          isText: false,
        }));
      } else {
        const data = await api.get("/api/children?path=" + encodeURIComponent(view.path));
        kids = normalizeChildren(data, view.path);
      }
    } catch (e) {
      error = errText(e);
    }
    const c = await ensureEngine(view);
    view.cosmos = c;
    if (c) {
      c.setScene({
        dirPath: view.path,
        children: kids,
        savedPositions: takeDirPositions(posBag(view), view.path),
        looks: state.starLooks,
        camera: opts.resetCamera ? undefined : view.camera,
        resetCamera: !!opts.resetCamera,
        error,
      });
      if (state.sortMode && state.sortMode !== "galaxy") c.arrangeBy(state.sortMode);
    }
    view.itemCount = kids.length;
    paintCrumbs(view);
    paintStatus();
    syncWatches();
  }

  async function navigateView(view, path, resetCamera, fromHist) {
    if (!path) return;
    if (!fromHist) {
      const hist = view.pathHist || (view.pathHist = [view.path]);
      const idx = view.pathIdx == null ? hist.length - 1 : view.pathIdx;
      view.pathHist = hist.slice(0, idx + 1);
      if (view.pathHist[view.pathHist.length - 1] !== path) view.pathHist.push(path);
      view.pathIdx = view.pathHist.length - 1;
    }
    if (view.cosmos && !resetCamera) view.camera = view.cosmos.getCamera();
    view.path = path;
    await loadChildren(view, { resetCamera: !!resetCamera });
    scheduleSave();
  }

  function hidePathGoto(view) {
    if (!view || !view.rootEl || view.cns) return;
    const el = view.rootEl.querySelector(".crumbs");
    if (!el) return;
    el.classList.remove("goto");
    const inp = el.querySelector(".crumb-path");
    if (inp) inp.value = view.path === THIS_PC ? "This PC" : view.path;
  }

  function paintCrumbs(view) {
    if (view.cns) {
      paintCnsCrumbs(view);
      return;
    }
    const el = view.rootEl.querySelector(".crumbs");
    el.innerHTML = "";
    el.classList.remove("goto");
    const parts = pathParts(view.path);
    const show =
      parts.length > 6
        ? [parts[0], { name: "…", path: null, overflow: parts.slice(1, -3) }].concat(parts.slice(-3))
        : parts;
    show.forEach((p, i) => {
      if (i) {
        const sep = document.createElement("span");
        sep.className = "crumb-sep";
        sep.textContent = "›";
        el.appendChild(sep);
      }
      const wrap = document.createElement("span");
      wrap.className = "crumb";
      const b = document.createElement("button");
      b.type = "button";
      b.textContent = p.name;
      if (p.overflow) {
        b.onclick = (e) => {
          const pop = $("#ctx");
          pop.hidden = false;
          pop.innerHTML = "";
          p.overflow.forEach((o) => {
            const it = document.createElement("button");
            it.type = "button";
            it.textContent = o.name;
            it.onclick = () => {
              hidePop();
              navigateView(view, o.path, true);
            };
            pop.appendChild(it);
          });
          placeCtx(e.clientX, e.clientY);
        };
      } else {
        b.onclick = () => navigateView(view, p.path, true);
      }
      wrap.appendChild(b);
      el.appendChild(wrap);
    });
    const go = document.createElement("input");
    go.className = "crumb-path";
    go.type = "text";
    go.spellcheck = false;
    go.value = view.path === THIS_PC ? "This PC" : view.path;
    go.title = "Type a path and press Enter";
    go.onkeydown = (e) => {
      if (e.key === "Escape") {
        e.preventDefault();
        hidePathGoto(view);
        go.blur();
        return;
      }
      if (e.key !== "Enter") return;
      e.preventDefault();
      const raw = go.value.trim();
      if (!raw) return;
      hidePathGoto(view);
      go.blur();
      if (/^this pc$/i.test(raw)) navigateView(view, THIS_PC, true);
      else navigateView(view, raw, true);
    };
    go.onblur = () => hidePathGoto(view);
    el.appendChild(go);
    paintPinStars();
  }

  function syncWatches() {
    const needed = new Set(
      state.views
        .filter((v) => v.path && v.path !== THIS_PC && !v.minimized && !v.cns)
        .map((v) => v.path)
    );
    for (const [path, es] of [...state.watchers]) {
      if (!needed.has(path)) {
        try {
          es.close();
        } catch (_) {}
        state.watchers.delete(path);
      }
    }
    needed.forEach((path) => {
      if (state.watchers.has(path)) return;
      try {
        const es = new EventSource("/api/watch?path=" + encodeURIComponent(path));
        es.onmessage = () => {
          state.views.filter((v) => v.path === path).forEach((v) => loadChildren(v));
        };
        es.onerror = () => {};
        state.watchers.set(path, es);
      } catch (_) {}
    });
  }

  function makeView(root, path, camera) {
    const v = {
      id: "v" + state.vid++,
      root: root || state.meta.defaultRoot || "",
      path: path || root || state.meta.defaultRoot || "",
      camera: camera || { x: 0, y: 0, scale: 1 },
      mode: "2d",
      selection: [],
      minimized: false,
      pathHist: [path || root || state.meta.defaultRoot || ""],
      pathIdx: 0,
      itemCount: 0,
    };
    state.views.push(v);
    return v;
  }

  function mountView(v, parent) {
    if (v.rootEl) {
      parent.appendChild(v.rootEl);
      if (v.cosmos2) v.cosmos2.resize();
      if (v.cosmos3) v.cosmos3.resize();
      return;
    }
    const el = document.createElement("div");
    el.className = "cview";
    el.dataset.id = v.id;
    el.innerHTML =
      '<header class="vhead"><div class="crumbs"></div><div class="vbtns">' +
      '<button type="button" data-act="pin-temp" title="Minimize">–</button>' +
      '<button type="button" data-act="close" title="Close">×</button>' +
      '<button type="button" data-act="pin-perm" title="Pin">☆</button>' +
      '<button type="button" data-act="split" title="New split (Ctrl+N)">⊞</button>' +
      '</div></header><div class="vstage"><div class="vcosmos"></div>' +
      '<div class="cns-inspect" hidden></div>' +
      '<div class="vdock" hidden><div class="dtabs"></div><div class="dbody"></div></div>' +
      '<div class="vmin" hidden>Minimized — restore from the pin bar</div></div>';
    parent.appendChild(el);
    v.rootEl = el;
    v.stage = el.querySelector(".vstage");
    el.addEventListener("pointerdown", () => focusView(v));
    el.querySelector('[data-act="pin-temp"]').onclick = (e) => {
      e.stopPropagation();
      tempPinView(v);
    };
    el.querySelector('[data-act="close"]').onclick = (e) => {
      e.stopPropagation();
      closeView(v);
    };
    el.querySelector('[data-act="pin-perm"]').onclick = (e) => {
      e.stopPropagation();
      togglePermPin({
        kind: "view",
        path: v.path,
        title: baseName(v.path) || v.path,
        root: v.root,
        camera: v.cosmos ? v.cosmos.getCamera() : v.camera,
      });
    };
    el.querySelector('[data-act="split"]').onclick = (e) => {
      e.stopPropagation();
      splitFocused(v);
    };
    v.cosmos2 = Cosmos2D.create(el.querySelector(".vcosmos"), hostHandlers(v));
    v.cosmos = v.cosmos2;
    const overlay = el.querySelector(".vmin");
    if (overlay) overlay.hidden = true;
    if (v.cns && !v.cns.session) {
      paintCrumbs(v);
      startCnsImportOverlay(v);
    } else {
      loadChildren(v);
    }
    paintPinStars();
    paintCloseButtons();
  }

  function renderSplit(node, host) {
    host.innerHTML = "";
    function paint(n, into) {
      if (n.type === "leaf") {
        const v = viewById(n.viewId);
        if (v && !v.minimized) mountView(v, into);
        return;
      }
      const axis = splitAxisOf(n);
      const row = document.createElement("div");
      row.className = axis === "y" ? "split-node axis-y" : "split-node col axis-x";
      const a = document.createElement("div");
      a.style.flex = String(n.ratio);
      a.style.minWidth = "0";
      a.style.minHeight = "0";
      a.style.display = "flex";
      const sash = document.createElement("div");
      sash.className = "sash";
      const b = document.createElement("div");
      b.style.flex = String(1 - n.ratio);
      b.style.minWidth = "0";
      b.style.minHeight = "0";
      b.style.display = "flex";
      row.append(a, sash, b);
      into.appendChild(row);
      paint(n.a, a);
      paint(n.b, b);
      sash.addEventListener("pointerdown", (e) => {
        const r = row.getBoundingClientRect();
        function mv(ev) {
          const t = axis === "y" ? (ev.clientY - r.top) / r.height : (ev.clientX - r.left) / r.width;
          n.ratio = Math.max(0.15, Math.min(0.85, t));
          a.style.flex = String(n.ratio);
          b.style.flex = String(1 - n.ratio);
        }
        function up() {
          window.removeEventListener("pointermove", mv);
          window.removeEventListener("pointerup", up);
          scheduleSave();
          resizeViews();
        }
        window.addEventListener("pointermove", mv);
        window.addEventListener("pointerup", up);
        e.preventDefault();
      });
    }
    paint(node, host);
  }

  function remount() {
    state.views.forEach((v) => {
      if (v.rootEl) v.rootEl.remove();
    });
    renderSplit(state.split, $("#splits"));
    focusView(latestUnminimized());
    paintPins();
    paintCloseButtons();
    requestAnimationFrame(() => {
      resizeViews();
      requestAnimationFrame(resizeViews);
    });
    setTimeout(resizeViews, 50);
    setTimeout(resizeViews, 200);
  }

  function paintCloseButtons() {
    state.views.forEach((v) => {
      const btn = v.rootEl && v.rootEl.querySelector('[data-act="close"]');
      if (!btn) return;
      const block = !v || v.minimized || !latestUnminimized(v);
      btn.disabled = block;
      btn.title = block ? "Cannot close the last view" : "Close (Ctrl+W)";
    });
  }

  function findLeaf(node, viewId) {
    if (!node) return null;
    if (node.type === "leaf") return node.viewId === viewId ? node : null;
    return findLeaf(node.a, viewId) || findLeaf(node.b, viewId);
  }

  function replaceNode(root, target, next) {
    if (root === target) return next;
    if (root.type !== "split") return root;
    root.a = replaceNode(root.a, target, next);
    root.b = replaceNode(root.b, target, next);
    return root;
  }

  function splitOffView(view, copyCns, openCns) {
    const v = view || focusedView();
    if (!v) return null;
    const folderPath = v.cns ? parentPath(v.cns.sdPath) || v.root : v.path;
    const camSrc = v.cosmos ? v.cosmos.getCamera() : v.camera;
    const nv = makeView(
      v.root,
      folderPath,
      copyCns && camSrc ? Object.assign({}, camSrc) : undefined
    );
    nv.mode = viewMode(v);
    if (copyCns && v.cns) {
      nv.cns = {
        session: v.cns.session,
        sdPath: v.cns.sdPath,
        stack: (v.cns.stack || []).slice(),
      };
      nv.path = v.path;
    } else if (openCns && openCns.path) {
      nv.cns = { session: 0, sdPath: openCns.path, stack: [] };
      nv.path = openCns.path;
    }
    const leaf = findLeaf(state.split, v.id);
    const neu = {
      type: "split",
      axis: splitAxisForPane(v),
      ratio: 0.5,
      a: { type: "leaf", viewId: v.id },
      b: { type: "leaf", viewId: nv.id },
    };
    if (!leaf) state.split = neu;
    else state.split = replaceNode(state.split, leaf, neu);
    remount();
    return nv;
  }

  function splitFocused(view) {
    const nv = splitOffView(view, true);
    if (!nv) return;
    scheduleSave();
  }

  function prune(node, viewId) {
    if (!node) return null;
    if (node.type === "leaf") return node.viewId === viewId ? null : node;
    const a = prune(node.a, viewId);
    const b = prune(node.b, viewId);
    if (!a) return b;
    if (!b) return a;
    return { type: "split", axis: splitAxisOf(node), ratio: node.ratio, a, b };
  }

  function closeView(v) {
    if (!v || v.minimized) return;
    if (!latestUnminimized(v)) return;
    state.pins = state.pins.filter((p) => !(p.kind === "view" && p.viewId === v.id && !p.permanent));
    discardView(v);
    if (!state.split) {
      let live = latestUnminimized();
      if (!live) {
        live = makeView(fillerRoot(), fillerRoot());
        live.filler = true;
      }
      state.split = { type: "leaf", viewId: live.id };
    }
    remount();
    focusView(latestUnminimized());
    syncWatches();
    scheduleSave();
  }

  function fillerRoot() {
    if (state.uiMode === "stardust") return sessionRoot();
    return state.meta.defaultRoot || "";
  }

  function closeFocused() {
    const win = SD.editor && SD.editor.focused();
    if (state.lastFocus === "win" && win && !win.minimized && (!win.el.hidden || win.docked)) {
      SD.editor.close(win.id);
      return;
    }
    closeView(focusedView());
  }

  function hidePop() {
    cancelFlyClose();
    $$(".menu-pop").forEach((p) => {
      if (p.id === "search-drop" || p.classList.contains("nested")) return;
      p.hidden = true;
    });
    $$(".menu-btn").forEach((b) => b.classList.remove("open"));
    const ctx = $("#ctx");
    ctx.hidden = true;
    ctx.innerHTML = "";
  }

  function placeCtx(x, y) {
    const ctx = $("#ctx");
    ctx.hidden = false;
    const r = ctx.getBoundingClientRect();
    let left = x;
    let top = y;
    if (left + r.width > innerWidth) left = innerWidth - r.width - 8;
    if (top + r.height > innerHeight) top = innerHeight - r.height - 8;
    if (left < 8) left = 8;
    if (top < 8) top = 8;
    ctx.style.left = left + "px";
    ctx.style.top = top + "px";
  }

  let flyCloseTimer = 0;

  function cancelFlyClose() {
    clearTimeout(flyCloseTimer);
    flyCloseTimer = 0;
  }

  function armFlyClose(fly) {
    cancelFlyClose();
    flyCloseTimer = setTimeout(() => fly.classList.remove("open"), 200);
  }

  function placeFlyout(fly, anchor) {
    const pad = 8;
    const overlap = 4;
    cancelFlyClose();
    fly.classList.add("open");
    fly.style.left = "0px";
    fly.style.top = "0px";
    fly.style.maxHeight = "";
    const ar = anchor.getBoundingClientRect();
    const fw = fly.offsetWidth;
    const fh = fly.scrollHeight;
    const maxH = Math.max(120, innerHeight - pad * 2);
    const useH = Math.min(fh, maxH);
    let left = ar.right - overlap;
    if (left + fw > innerWidth - pad) left = ar.left - fw + overlap;
    if (left < pad) left = pad;
    if (left + fw > innerWidth - pad) left = Math.max(pad, innerWidth - pad - fw);
    let top = ar.top;
    if (top + useH > innerHeight - pad) top = innerHeight - pad - useH;
    if (top < pad) top = pad;
    fly.style.left = left + "px";
    fly.style.top = top + "px";
    fly.style.maxHeight = maxH + "px";
  }

  function fillMenu(parent, items) {
    items.forEach((it) => {
      if (it === "hr") {
        parent.appendChild(document.createElement("hr"));
        return;
      }
      if (it.items) {
        const wrap = document.createElement("div");
        wrap.className = "submenu";
        const btn = document.createElement("button");
        btn.type = "button";
        btn.className = "submenu-btn";
        btn.textContent = it.label;
        const nested = document.createElement("div");
        nested.className = "menu-pop nested";
        fillMenu(nested, it.items);
        wrap.appendChild(btn);
        wrap.appendChild(nested);
        wrap.addEventListener("mouseenter", () => placeFlyout(nested, wrap));
        wrap.addEventListener("mouseleave", () => armFlyClose(nested));
        btn.addEventListener("click", (ev) => {
          ev.stopPropagation();
          placeFlyout(nested, wrap);
        });
        parent.appendChild(wrap);
        return;
      }
      const b = document.createElement("button");
      b.type = "button";
      b.textContent = it.label;
      b.disabled = !!it.off;
      b.onclick = () => {
        if (it.fn && !it.off) it.fn();
        hidePop();
      };
      parent.appendChild(b);
    });
  }

  function menuItems(items) {
    const ctx = $("#ctx");
    ctx.innerHTML = "";
    fillMenu(ctx, items);
  }

  async function showExplorerMenu(e, view, star) {
    if (view.cns) {
      showCnsMenu(e, view, star);
      return;
    }
    const group = actionStars(star);
    const dirs = group.filter((s) => s.kind === "dir");
    const files = group.filter((s) => s.kind !== "dir");
    const items = [];
    if (group.length) {
      const openLabel =
        group.length > 1 ? "Open " + group.length : "Open";
      items.push({ label: openLabel, fn: () => openStars(view, group) });
      items.push({
        label: group.length === 1 && files.length === 1 ? "Show in Explorer" : "Open in Explorer",
        fn: () => openInExplorerOs(group[0] || { path: view.path }),
      });
      if (dirs.length) {
        items.push({
          label: dirs.length > 1 ? "Open " + dirs.length + " in new splits" : "Open in new split",
          fn: () => openStarsInNewSplits(view, group),
        });
      }
      if (group.length === 1 && files.length === 1) {
        let inspectOff = true;
        let inspectLabel = "Inspect IR";
        let inspectData = null;
        try {
          inspectData = await api.get("/api/inspect?path=" + encodeURIComponent(files[0].path));
          inspectOff = !inspectData || inspectData.ok === false;
          if (inspectOff) inspectLabel = "Inspect IR — " + ((inspectData && inspectData.error) || "not a StarDust layout");
        } catch (err) {
          inspectOff = true;
          inspectLabel = "Inspect IR — " + errText(err);
        }
        items.push({
          label: inspectLabel,
          off: inspectOff,
          fn: () => inspectPath(files[0].path, inspectData),
        });
        let cnsOff = true;
        let cnsLabel = "Inspect in StarDust";
        try {
          const st = await api.get("/api/cns/status");
          if (!st || !st.python) {
            cnsLabel = "Inspect in StarDust — " + ((st && st.reason) || "needs --features python");
          } else {
            const pr = await api.get("/api/cns/probe?path=" + encodeURIComponent(files[0].path));
            cnsOff = !pr || !pr.canInspect;
            if (cnsOff)
              cnsLabel =
                "Inspect in StarDust — " + ((pr && pr.reason) || "no instance illustration");
          }
        } catch (err) {
          cnsOff = true;
          cnsLabel = "Inspect in StarDust — " + errText(err);
        }
        items.push({
          label: cnsLabel,
          off: cnsOff,
          fn: () => inspectInStarDust(view, files[0].path),
        });
      }
      const pinSpecs = group.map((s) => ({
        kind: s.kind === "dir" ? "view" : "file",
        path: s.path,
      }));
      const allPinned = pinSpecs.every((s) => permPinFor(s.kind, s.path));
      items.push({
        label: allPinned
          ? group.length > 1
            ? "Unpin " + group.length
            : "Unpin"
          : group.length > 1
            ? "Pin " + group.length
            : "Pin",
        fn: () => pinStars(view, group),
      });
      items.push("hr");
    }
    if (!star) {
      items.push({
        label: "Open in Explorer",
        fn: () => openInExplorerOs({ path: view.path }),
      });
      items.push({ label: "New file", fn: () => newEntry(view, "file") });
      items.push({ label: "New folder", fn: () => newEntry(view, "dir") });
    }
    if (group.length) {
      if (!star) items.push("hr");
      items.push({
        label: group.length > 1 ? "Send " + group.length + " to Agent" : "Send to Agent",
        fn: () => attachStars(group),
      });
      items.push({ label: group.length > 1 ? "Cut " + group.length : "Cut", fn: () => clip("cut", group) });
      items.push({ label: group.length > 1 ? "Copy " + group.length : "Copy", fn: () => clip("copy", group) });
    }
    items.push({ label: "Paste", off: !state.fileClip, fn: () => pasteInto(view) });
    if (group.length) {
      items.push({
        label: group.length > 1 ? "Copy " + group.length + " paths" : "Copy path",
        fn: () => navigator.clipboard.writeText(group.map((s) => s.path).join("\n")).catch(() => {}),
      });
      items.push({
        label: "Rename",
        off: group.length > 1,
        fn: () => startRename(view, group[0]),
      });
      items.push({
        label: group.length > 1 ? "Delete " + group.length : "Delete",
        fn: () => deleteStars(view, group),
      });
      items.push("hr");
      items.push({ label: "Properties", fn: () => showProps(group.length === 1 ? group[0] : star || group[0]) });
      if (group.length === 1) {
        items.push("hr");
        lookMenuItems(group[0]).forEach((it) => items.push(it));
      }
    }
    menuItems(items);
    placeCtx(e.clientX, e.clientY);
  }

  function openInExplorerOs(star) {
    const p = star && star.path;
    if (!p || p === THIS_PC) return;
    api.post("/api/open-explorer", { path: p }).catch((e) => toast(errText(e)));
  }

  function isShortcut(star) {
    const n = (star && (star.name || star.path)) || "";
    return /\.lnk$/i.test(n);
  }

  function openStar(view, star) {
    if (!star) return;
    if (view.cns) {
      openCnsStar(view, star);
      return;
    }
    if (isShortcut(star)) {
      api.post("/api/open-external", { path: star.path }).catch((e) => toast(errText(e)));
      return;
    }
    if (star.kind === "dir") navigateView(view, star.path, true);
    else if (SD.editor) SD.editor.open(star.path, { isText: star.isText });
  }

  async function openInNewSplit(view, star) {
    if (!view || !star || star.kind !== "dir") return;
    splitFocused(view);
    const nv = state.views[state.views.length - 1];
    if (!nv) return;
    nv.root = view.root;
    await navigateView(nv, star.path, true);
  }

  async function openStars(view, stars) {
    const list = (stars || []).filter(Boolean);
    if (!list.length) return;
    const dirs = list.filter((s) => s.kind === "dir");
    const files = list.filter((s) => s.kind !== "dir");
    files.forEach((s) => {
      if (isShortcut(s)) api.post("/api/open-external", { path: s.path }).catch((e) => toast(errText(e)));
      else if (SD.editor) SD.editor.open(s.path, { isText: s.isText });
    });
    if (!dirs.length) return;
    if (dirs.length === 1 && !files.length) {
      await navigateView(view, dirs[0].path, true);
      return;
    }
    for (const d of dirs) await openInNewSplit(view, d);
  }

  async function openStarsInNewSplits(view, stars) {
    const dirs = (stars || []).filter((s) => s && s.kind === "dir");
    for (const d of dirs) await openInNewSplit(view, d);
  }

  function pinStars(view, stars) {
    const list = (stars || []).filter((s) => s && s.path);
    if (!list.length) return;
    const specs = list.map((s) => ({
      kind: s.kind === "dir" ? "view" : "file",
      path: s.path,
      title: s.name,
      root: view.root,
    }));
    const allOn = specs.every((s) => permPinFor(s.kind, s.path));
    specs.forEach((s) => {
      const ex = permPinFor(s.kind, s.path);
      if (allOn) {
        if (ex) unpin(ex);
      } else if (!ex) {
        togglePermPin(s);
      }
    });
  }

  function clip(mode, stars) {
    state.fileClip = { mode, paths: stars.map((s) => s.path) };
    refreshEditMenu();
  }

  async function mutate(kind, body) {
    try {
      await api.post("/api/" + kind, body);
      return true;
    } catch (e) {
      toast(kind + " failed: " + errText(e));
      return false;
    }
  }

  function placeNearCursor(view, name, kind) {
    const c = cosmosOf(view);
    const w = state.lastViewId === view.id ? state.lastWorld : c && c.placeHint ? c.placeHint() : { x: 0, y: 0 };
    if (c && c.notePending) c.notePending(name, w.x, w.y, kind);
  }

  async function pasteInto(view) {
    const clipd = state.fileClip;
    if (!clipd) return;
    for (const from of clipd.paths) {
      const name = baseName(from);
      const to = joinPath(view.path, name);
      placeNearCursor(view, name);
      const ok = await mutate(clipd.mode === "cut" ? "move" : "copy", { from, to });
      if (!ok) break;
    }
    if (clipd.mode === "cut") state.fileClip = null;
    await loadChildren(view);
  }

  async function newEntry(view, kind) {
    const name = await promptText(
      kind === "dir" ? "New folder name" : "New file name",
      kind === "dir" ? "untitled" : "untitled.txt"
    );
    if (!name) return;
    const path = joinPath(view.path, name);
    placeNearCursor(view, name, kind);
    const ok =
      kind === "dir" ? await mutate("mkdir", { path }) : await mutate("write-new", { path, content: "" });
    if (ok) await loadChildren(view);
  }

  async function deleteStars(view, stars) {
    if (!stars.length) return;
    const ok = await confirmText(
      "Delete",
      "Send " + stars.map((s) => s.name).join(", ") + " to Recycle Bin / Trash?"
    );
    if (!ok) return;
    for (const s of stars) await mutate("delete", { path: s.path });
    await loadChildren(view);
  }

  function startRename(view, star) {
    const c = cosmosOf(view);
    if (!c || !star) return;
    const inp = $("#rename-overlay");
    const scr = c.screenOf(star);
    inp.hidden = false;
    inp.value = star.name;
    inp.style.left = scr.x - 40 + "px";
    inp.style.top = scr.y + scr.r + 6 + "px";
    inp.style.width = Math.max(80, star.name.length * 8 + 24) + "px";
    inp.focus();
    inp.select();
    let finished = false;
    const finish = async (commit) => {
      if (finished) return;
      finished = true;
      inp.hidden = true;
      inp.onkeydown = null;
      inp.onblur = null;
      if (!commit) return;
      const name = inp.value.trim();
      if (!name || name === star.name) return;
      const to = joinPath(parentPath(star.path) || view.path, name);
      const ok = await mutate("rename", { from: star.path, to });
      if (ok) await loadChildren(view);
    };
    inp.onkeydown = (e) => {
      if (e.key === "Enter") {
        e.preventDefault();
        finish(true);
      }
      if (e.key === "Escape") finish(false);
    };
    inp.onblur = () => finish(true);
  }

  async function showProps(star) {
    let body = { name: star.name, path: star.path, kind: star.kind, size: star.size };
    try {
      const st = await api.get("/api/stat?path=" + encodeURIComponent(star.path));
      body = st || body;
    } catch (_) {}
    modal({
      title: "Properties",
      html: '<pre class="stat-pre"></pre>',
      buttons: [{ label: "Close", primary: true }],
    });
    const pre = $("#modal-body").querySelector(".stat-pre");
    if (pre) pre.textContent = JSON.stringify(body, null, 2);
  }

  function modal({ title, html, buttons }) {
    return new Promise((resolve) => {
      const overlay = $("#modal");
      $("#modal-title").textContent = title || "";
      $("#modal-body").innerHTML = html || "";
      const acts = $("#modal-actions");
      acts.innerHTML = "";
      overlay.hidden = false;
      (buttons || [{ label: "OK", primary: true, value: true }]).forEach((b) => {
        const btn = document.createElement("button");
        btn.type = "button";
        btn.textContent = b.label;
        if (b.primary) btn.className = "primary";
        btn.onclick = () => {
          overlay.hidden = true;
          resolve(b.value);
        };
        acts.appendChild(btn);
      });
    });
  }

  async function promptText(title, value) {
    const p = modal({
      title,
      html: '<input id="modal-input" />',
      buttons: [
        { label: "Cancel", value: null },
        { label: "OK", primary: true, value: "ok" },
      ],
    });
    const inp = $("#modal-input");
    inp.value = value || "";
    setTimeout(() => inp.focus(), 0);
    const v = await p;
    if (v !== "ok") return null;
    return inp.value.trim();
  }

  function confirmText(title, msg) {
    return modal({
      title,
      html: "<p>" + msg.replace(/</g, "&lt;") + "</p>",
      buttons: [
        { label: "Cancel", value: false },
        { label: "Delete", primary: true, value: true },
      ],
    });
  }

  function confirmDirty(title) {
    return modal({
      title: "Unsaved changes",
      html: "<p>Save changes to " + String(title).replace(/</g, "&lt;") + "?</p>",
      buttons: [
        { label: "Cancel", value: "cancel" },
        { label: "Don't save", value: "discard" },
        { label: "Save", primary: true, value: "save" },
      ],
    });
  }

  function confirmChoice(title, msg, okLabel) {
    return modal({
      title,
      html: "<p>" + String(msg || "").replace(/</g, "&lt;") + "</p>",
      buttons: [
        { label: "Cancel", value: false },
        { label: okLabel || "OK", primary: true, value: true },
      ],
    });
  }

  async function handleDrop(e, view) {
    const dt = e.dataTransfer;
    if (!dt) return;
    const shift = e.shiftKey;
    const ctrl = e.ctrlKey || e.metaKey;
    const move = shift && !ctrl;
    if ((dt.types || []).indexOf("text/stardust-path") >= 0) {
      const from = dt.getData("text/stardust-path");
      const to = joinPath(view.path, baseName(from));
      placeNearCursor(view, baseName(from));
      await mutate(move ? "move" : "copy", { from, to });
      await loadChildren(view);
      return;
    }
    const files = [...(dt.files || [])];
    for (const f of files) {
      const from = f.path;
      const dest = joinPath(view.path, f.name);
      placeNearCursor(view, f.name);
      if (from) {
        await mutate(move ? "move" : "copy", { from, to: dest });
      } else {
        try {
          const text = await f.text();
          await mutate("write-new", { path: dest, content: text || "" });
        } catch (_) {
          toast("Browser did not provide a path for " + f.name + "; binary drop skipped.");
        }
      }
    }
    if (files.length) await loadChildren(view);
  }

  let pinSuppressClick = false;
  let pinDidReorder = false;

  function pinGroupKey(p) {
    return p.kind + ":" + pathKey(p.path || p.title || "");
  }

  function collectPinGroups() {
    const visFiles = new Set(
      (SD.editor ? SD.editor.windows() : [])
        .filter((w) => !w.minimized && w.el && !w.el.hidden)
        .map((w) => w.path)
    );
    const groups = [];
    const byKey = new Map();
    const leftover = [];
    state.pins.forEach((p) => {
      if (!p.permanent && !p.minimized) {
        leftover.push(p);
        return;
      }
      if (p.kind === "file" && visFiles.has(p.path) && !p.minimized && !p.permanent) {
        leftover.push(p);
        return;
      }
      const key = pinGroupKey(p);
      let g = byKey.get(key);
      if (!g) {
        g = { key, kind: p.kind, pins: [] };
        byKey.set(key, g);
        groups.push(g);
      }
      g.pins.push(p);
    });
    return { groups, leftover };
  }

  function reorderPinGroups(fromKey, toKey, before) {
    if (!fromKey || !toKey || fromKey === toKey) return;
    const { groups, leftover } = collectPinGroups();
    const iFrom = groups.findIndex((g) => g.key === fromKey);
    if (iFrom < 0) return;
    const [g] = groups.splice(iFrom, 1);
    const iTo = groups.findIndex((x) => x.key === toKey);
    if (iTo < 0) groups.push(g);
    else groups.splice(before ? iTo : iTo + 1, 0, g);
    state.pins = groups.flatMap((x) => x.pins).concat(leftover);
    pinDidReorder = true;
    paintPins();
    scheduleSave();
  }

  function paintPins() {
    const bar = $("#pin");
    bar.innerHTML = "";
    const { groups } = collectPinGroups();
    groups.forEach((g) => {
      const temps = g.pins.filter((p) => !p.permanent && p.minimized);
      const perm = g.pins.find((p) => p.permanent);
      const n = temps.length;
      const src = g.pins[0];
      const title = src.title || src.path || "pin";
      const b = document.createElement("button");
      b.type = "button";
      b.className = perm ? "perm" : "temp";
      b.draggable = true;
      b.dataset.pinKey = g.key;
      const star = document.createElement("span");
      star.className = "pin-star";
      star.textContent = perm ? "★" : "☆";
      const label = n > 1 ? title + " (" + n + ")" : title;
      b.append(star, document.createTextNode(label));
      b.title = perm ? "Open · Ctrl+click to unpin" : "Restore";
      b.onclick = (e) => {
        if (pinSuppressClick) {
          pinSuppressClick = false;
          return;
        }
        if ((e.ctrlKey || e.metaKey) && perm) {
          e.preventDefault();
          unpin(perm);
          return;
        }
        if (temps.length) restorePin(temps[temps.length - 1]);
        else if (perm) restorePin(perm);
      };
      b.addEventListener("dragstart", (ev) => {
        pinDidReorder = false;
        ev.dataTransfer.setData("text/plain", g.key);
        ev.dataTransfer.effectAllowed = "move";
        b.classList.add("dragging");
      });
      b.addEventListener("dragend", () => {
        b.classList.remove("dragging");
        $$("#pin button").forEach((x) => {
          x.classList.remove("pin-drop-before", "pin-drop-after");
        });
        if (pinDidReorder) pinSuppressClick = true;
        pinDidReorder = false;
      });
      b.addEventListener("dragover", (ev) => {
        ev.preventDefault();
        ev.dataTransfer.dropEffect = "move";
        const r = b.getBoundingClientRect();
        const dropBefore = ev.clientX < r.left + r.width / 2;
        $$("#pin button").forEach((x) => {
          x.classList.remove("pin-drop-before", "pin-drop-after");
        });
        b.classList.add(dropBefore ? "pin-drop-before" : "pin-drop-after");
      });
      b.addEventListener("drop", (ev) => {
        ev.preventDefault();
        ev.stopPropagation();
        const from = ev.dataTransfer.getData("text/plain") || "";
        const r = b.getBoundingClientRect();
        const dropBefore = ev.clientX < r.left + r.width / 2;
        reorderPinGroups(from, g.key, dropBefore);
      });
      bar.appendChild(b);
    });
    paintPinStars();
  }

  function permPinFor(kind, path) {
    return state.pins.find((p) => p.permanent && p.kind === kind && pathKey(p.path) === pathKey(path));
  }

  function paintPinStars() {
    state.views.forEach((v) => {
      const btn = v.rootEl && v.rootEl.querySelector('[data-act="pin-perm"]');
      if (!btn) return;
      const on = !!permPinFor("view", v.path);
      btn.textContent = on ? "★" : "☆";
      btn.classList.toggle("pinned", on);
      btn.title = on ? "Unpin" : "Pin";
    });
    $$("[data-act='pin-file']").forEach((btn) => {
      const on = !!permPinFor("file", btn.dataset.path);
      btn.textContent = on ? "★" : "☆";
      btn.classList.toggle("pinned", on);
      btn.title = on ? "Unpin" : "Pin";
    });
  }

  function unpin(p) {
    state.pins = state.pins.filter((x) => x !== p);
    paintPins();
    scheduleSave();
  }

  function togglePermPin(spec) {
    if (!spec || !spec.path) return;
    const kind = spec.kind || "view";
    const existing = permPinFor(kind, spec.path);
    if (existing) {
      unpin(existing);
      return;
    }
    state.pins.push({
      id: "p" + Date.now(),
      kind,
      permanent: true,
      minimized: false,
      title: spec.title || baseName(spec.path) || spec.path,
      root: spec.root,
      path: spec.path,
      camera: spec.camera,
    });
    paintPins();
    scheduleSave();
  }

  function discardView(v) {
    if (!v) return;
    stopCnsImportOverlay(v);
    state.split = prune(state.split, v.id);
    state.views = state.views.filter((x) => x.id !== v.id);
    state.focusHist = (state.focusHist || []).filter((id) => id !== v.id);
    if (v.cosmos2) v.cosmos2.destroy();
    if (v.cosmos3) v.cosmos3.destroy();
    if (v.rootEl) v.rootEl.remove();
  }

  function tempPinView(v) {
    if (!v || v.minimized) return;
    if (v.cosmos) v.camera = v.cosmos.getCamera();
    v.minimized = true;
    const overlay = v.rootEl && v.rootEl.querySelector(".vmin");
    if (overlay) overlay.hidden = true;
    if (!state.pins.some((p) => !p.permanent && p.kind === "view" && p.viewId === v.id)) {
      state.pins.push({
        id: "t" + v.id,
        kind: "view",
        permanent: false,
        minimized: true,
        title: baseName(v.path) || v.path,
        viewId: v.id,
        root: v.root,
        path: v.path,
      });
    }
    const next = latestUnminimized(v);
    state.split = prune(state.split, v.id);
    if (!state.split) {
      let live = next;
      if (!live) {
        live = makeView(fillerRoot(), fillerRoot());
        live.filler = true;
      }
      state.split = { type: "leaf", viewId: live.id };
    }
    remount();
    const focus = findLeaf(state.split, next && next.id) ? next : latestUnminimized();
    focusView(focus);
    syncWatches();
    scheduleSave();
  }

  function permPinStar(star) {
    togglePermPin({
      kind: star.kind === "dir" ? "view" : "file",
      path: star.path,
      title: star.name,
      root: (focusedView() || {}).root,
    });
  }

  function tempPinWindow(w) {
    SD.editor.hide(w);
    if (!state.pins.some((p) => !p.permanent && p.kind === "file" && p.winId === w.id)) {
      state.pins.push({
        id: "tw" + w.id,
        kind: "file",
        permanent: false,
        minimized: true,
        title: w.title,
        path: w.path,
        winId: w.id,
      });
    }
    paintPins();
    scheduleSave();
  }

  function restoreMinimizedView(v) {
    if (!v) return;
    v.minimized = false;
    v.filler = false;
    const overlay = v.rootEl && v.rootEl.querySelector(".vmin");
    if (overlay) overlay.hidden = true;
    const pin = state.pins.find((p) => p.kind === "view" && p.viewId === v.id && p.minimized && !p.permanent);
    if (pin) state.pins = state.pins.filter((x) => x !== pin);
    if (!findLeaf(state.split, v.id)) {
      const cur = focusedView();
      if (cur && cur.filler && cur.id !== v.id) {
        discardView(cur);
        state.split = state.split || { type: "leaf", viewId: v.id };
      }
      if (!findLeaf(state.split, v.id)) {
        const host = focusedView();
        const leaf = host && findLeaf(state.split, host.id);
        if (leaf) {
          state.split = replaceNode(state.split, leaf, {
            type: "split",
            axis: splitAxisForPane(host),
            ratio: 0.5,
            a: { type: "leaf", viewId: host.id },
            b: { type: "leaf", viewId: v.id },
          });
        } else if (!state.split) {
          state.split = { type: "leaf", viewId: v.id };
        } else {
          state.split = {
            type: "split",
            axis: splitAxisForPane(null),
            ratio: 0.5,
            a: state.split,
            b: { type: "leaf", viewId: v.id },
          };
        }
      }
    }
    remount();
    focusView(v);
    paintPins();
    scheduleSave();
  }

  function restorePin(p) {
    if (p.kind === "file") {
      if (p.winId && SD.editor) {
        const w = SD.editor.windows().find((x) => x.id === p.winId);
        if (w) SD.editor.show(w);
        else SD.editor.open(p.path);
      } else SD.editor.open(p.path);
      if (!p.permanent) {
        p.minimized = false;
        state.pins = state.pins.filter((x) => x !== p);
      }
    } else if (p.kind === "view") {
      const byId = p.viewId ? viewById(p.viewId) : null;
      const hidden =
        (byId && byId.minimized && byId) ||
        state.views.find((x) => x.minimized && pathKey(x.path) === pathKey(p.path));
      if (hidden) {
        restoreMinimizedView(hidden);
        if (p.permanent) p.viewId = undefined;
      } else {
        const v = focusedView();
        if (v) {
          v.root = p.root || v.root;
          navigateView(v, p.path, true);
        }
      }
      if (!p.permanent) state.pins = state.pins.filter((x) => x !== p);
      else p.minimized = false;
    }     else if (p.kind === "inspect") inspectPath(p.path);
    else if (p.kind === "test") runCargoTest();
    else if (p.kind === "rebuild") runRebuildUi();
    paintPins();
    syncWatches();
    scheduleSave();
  }

  function prettyUl(node) {
    const li = document.createElement("li");
    const name = document.createElement("span");
    name.textContent = node && node.name != null ? String(node.name) : "value";
    li.appendChild(name);
    if (node && node.kind) {
      const k = document.createElement("span");
      k.className = "k";
      k.textContent = " : " + node.kind;
      li.appendChild(k);
    }
    const kids = node && node.children;
    if (kids && kids.length) {
      const ul = document.createElement("ul");
      kids.forEach((c) => ul.appendChild(prettyUl(c)));
      li.appendChild(ul);
    } else if (node && typeof node === "object" && !node.name && !Array.isArray(node)) {
      const ul = document.createElement("ul");
      Object.entries(node).forEach(([k, v]) => {
        if (k === "children") return;
        ul.appendChild(prettyUl(typeof v === "object" && v ? Object.assign({ name: k }, v) : { name: k, kind: String(v) }));
      });
      if (ul.childNodes.length) li.appendChild(ul);
    }
    return li;
  }

  function wireFloat(el) {
    const title = el.querySelector(".float-title");
    const rz = el.querySelector(".float-resize");
    title.addEventListener("pointerdown", (e) => {
      if (e.target.closest("button")) return;
      const r = el.getBoundingClientRect();
      const main = $("#main").getBoundingClientRect();
      const ox = r.left - main.left;
      const oy = r.top - main.top;
      const sx = e.clientX;
      const sy = e.clientY;
      function mv(ev) {
        el.style.left = ox + ev.clientX - sx + "px";
        el.style.top = oy + ev.clientY - sy + "px";
      }
      function up() {
        window.removeEventListener("pointermove", mv);
        window.removeEventListener("pointerup", up);
      }
      window.addEventListener("pointermove", mv);
      window.addEventListener("pointerup", up);
    });
    if (rz) {
      rz.addEventListener("pointerdown", (e) => {
        e.stopPropagation();
        const r = el.getBoundingClientRect();
        const sx = e.clientX;
        const sy = e.clientY;
        const ow = r.width;
        const oh = r.height;
        function mv(ev) {
          el.style.width = Math.max(280, ow + ev.clientX - sx) + "px";
          el.style.height = Math.max(140, oh + ev.clientY - sy) + "px";
        }
        function up() {
          window.removeEventListener("pointermove", mv);
          window.removeEventListener("pointerup", up);
        }
        window.addEventListener("pointermove", mv);
        window.addEventListener("pointerup", up);
      });
    }
  }

  async function inspectPath(path, data) {
    if (!data) {
      try {
        data = await api.get("/api/inspect?path=" + encodeURIComponent(path));
      } catch (e) {
        toast("Inspect: " + errText(e));
        data = { ok: false, error: errText(e) };
      }
    }
    if (data && data.ok === false) {
      toast(data.error || "not a StarDust layout");
    }
    const floats = $("#floats");
    const el = document.createElement("div");
    el.className = "float-win focused";
    el.style.left = "48px";
    el.style.top = "40px";
    el.style.width = "520px";
    el.style.height = "420px";
    const dump = data && (data.ir || data.dumpable || data.dump);
    el.innerHTML =
      '<div class="float-title"><span class="t">Inspect ' +
      baseName(path).replace(/</g, "") +
      '</span><button type="button" data-act="pin" title="Pin">☆</button><button type="button" data-act="x">×</button></div>' +
      '<div class="inspect-bar"><label><input type="checkbox" class="ir-tog"/> Dumpable IR</label></div>' +
      '<div class="float-body"><div class="inspect-tree"></div><textarea class="inspect-ir" hidden></textarea></div>' +
      '<div class="float-resize"></div>';
    floats.appendChild(el);
    const tree = el.querySelector(".inspect-tree");
    const ta = el.querySelector(".inspect-ir");
    ta.value = typeof dump === "string" ? dump : dump ? JSON.stringify(dump, null, 2) : "";
    const src = (data && (data.pretty || data.tree || data.fields || data.layout)) || {
      name: baseName(path),
      kind: (data && data.error) || "not a StarDust layout",
    };
    const ul = document.createElement("ul");
    ul.appendChild(prettyUl(src));
    tree.appendChild(ul);
    el.querySelector(".ir-tog").onchange = (e) => {
      ta.hidden = !e.target.checked;
      tree.style.display = e.target.checked ? "none" : "block";
    };
    el.querySelector('[data-act="x"]').onclick = () => el.remove();
    const pinBtn = el.querySelector('[data-act="pin"]');
    function paintInspPin() {
      const on = !!permPinFor("inspect", path);
      pinBtn.textContent = on ? "★" : "☆";
      pinBtn.title = on ? "Unpin" : "Pin";
    }
    pinBtn.onclick = () => {
      togglePermPin({ kind: "inspect", path, title: "Inspect " + baseName(path) });
      paintInspPin();
    };
    paintInspPin();
    wireFloat(el);
  }

  function showCnsMenu(e, view, star) {
    const items = [];
    if (star && star.role !== "cluster") {
      items.push({ label: "Enter neighborhood", fn: () => openCnsStar(view, star) });
    } else if (star && star.role === "cluster") {
      items.push({ label: "Zoom in", fn: () => openCnsStar(view, star) });
    }
    if (view.cns && view.cns.stack && view.cns.stack.length) {
      items.push({ label: "Up to plane", fn: () => cnsLeave(view) });
    } else {
      items.push({ label: "Leave illustration", fn: () => cnsLeave(view) });
    }
    menuItems(items);
    placeCtx(e.clientX, e.clientY);
    $("#ctx").hidden = false;
  }

  function cnsImportMessage(view) {
    const t0 = (view && view.cnsImportAt) || Date.now();
    const secs = Math.max(0, Math.floor((Date.now() - t0) / 1000));
    return "Inspect in StarDust — importing Word… " + secs + "s";
  }

  function paintCnsImportStatus(view) {
    if (!view) return;
    const msg = cnsImportMessage(view);
    const apply = (c) => {
      if (!c) return;
      if (c.setScene) {
        c.setScene({
          loading: true,
          children: [],
          loadText: msg,
          loadClass: "vload cns-load",
        });
      } else if (c.statusEl) {
        c.statusEl.hidden = false;
        c.statusEl.className = "vload cns-load";
        c.statusEl.textContent = msg;
      }
    };
    apply(view.cosmos);
    if (view.cosmos2 && view.cosmos2 !== view.cosmos) apply(view.cosmos2);
    if (view.cosmos3 && view.cosmos3 !== view.cosmos) apply(view.cosmos3);
  }

  function startCnsImportOverlay(view) {
    if (!view) return;
    if (!view.cnsImportAt) view.cnsImportAt = Date.now();
    paintCnsImportStatus(view);
    if (view.cnsImportTimer) return;
    view.cnsImportTimer = setInterval(() => paintCnsImportStatus(view), 1000);
  }

  function stopCnsImportOverlay(view) {
    if (!view) return;
    if (view.cnsImportTimer) {
      clearInterval(view.cnsImportTimer);
      view.cnsImportTimer = null;
    }
    view.cnsImportAt = 0;
  }

  async function inspectInStarDust(view, path) {
    const src = view || focusedView();
    if (!src || !path) return;
    const nv = splitOffView(src, false, { path: path });
    if (!nv) return;
    focusView(nv);
    startCnsImportOverlay(nv);
    await fillCnsView(nv, path);
  }

  async function fillCnsView(view, path) {
    if (!view || !path) return;
    if (!view.cns) view.cns = { session: 0, sdPath: path, stack: [] };
    startCnsImportOverlay(view);
    const c0 = await ensureEngine(view);
    view.cosmos = c0;
    paintCnsImportStatus(view);
    paintCrumbs(view);
    try {
      const data = await api.post("/api/cns/open", { path: path });
      if (!data || data.ok === false) {
        stopCnsImportOverlay(view);
        delete view.cns;
        toast((data && data.error) || "Inspect in StarDust failed");
        if (c0 && c0.setScene) c0.setScene({ dirPath: path, children: [], error: (data && data.error) || "failed" });
        paintCrumbs(view);
        return;
      }
      view.cns = {
        session: data.session,
        sdPath: path,
        stack: [],
      };
      view.path = path;
      const hist = view.pathHist || (view.pathHist = []);
      if (hist[hist.length - 1] !== path) hist.push(path);
      view.pathIdx = hist.length - 1;
      await loadCnsStars(view, { resetCamera: true });
      focusView(view);
    } catch (e) {
      stopCnsImportOverlay(view);
      delete view.cns;
      toast("Inspect in StarDust: " + errText(e));
      if (c0 && c0.setScene)
        c0.setScene({ dirPath: path, children: [], error: errText(e) });
      paintCrumbs(view);
    }
  }

  function cnsPaneSize(view, c) {
    const pane = (view && view.stage) || (c && c.canvas && c.canvas.parentElement);
    const rect = pane && pane.getBoundingClientRect ? pane.getBoundingClientRect() : null;
    return {
      w: Math.max(1, (rect && rect.width) || 800),
      h: Math.max(1, (rect && rect.height) || 600),
    };
  }

  function cnsCamFromSpan(view, c, cx, cy, bw, bh, scaleMin, scaleMax, rMin, rMul) {
    const pane = cnsPaneSize(view, c);
    const margin = 1.2;
    const scale = Math.min(pane.w / (bw * margin), pane.h / (bh * margin));
    const sc = Math.max(scaleMin, Math.min(scaleMax, scale));
    const span = Math.max(bw, bh);
    return {
      x: cx,
      y: cy,
      scale: sc,
      tx: cx,
      tz: cy,
      r: Math.max(rMin, span * rMul),
    };
  }

  function frameCnsCloud(view, c, kids) {
    if (!c || !c.setCamera) return;
    let minx = Infinity;
    let maxx = -Infinity;
    let miny = Infinity;
    let maxy = -Infinity;
    for (let i = 0; i < kids.length; i++) {
      const s = kids[i];
      const x = +s.x || 0;
      const y = +s.y || 0;
      if (x < minx) minx = x;
      if (x > maxx) maxx = x;
      if (y < miny) miny = y;
      if (y > maxy) maxy = y;
    }
    if (!isFinite(minx)) {
      c.setCamera({ x: 0, y: 0, scale: 0.75 });
      return;
    }
    const cx = (minx + maxx) * 0.5;
    const cy = (miny + maxy) * 0.5;
    const bw = Math.max(12, maxx - minx);
    const bh = Math.max(12, maxy - miny);
    c.setCamera(cnsCamFromSpan(view, c, cx, cy, bw, bh, 0.4, 8, 56, 2.4));
  }

  function cnsJumpCam(view, c, kids, jumpName) {
    kids = kids || [];
    let target = null;
    for (let i = 0; i < kids.length; i++) {
      if (kids[i].name === jumpName) {
        target = kids[i];
        break;
      }
    }
    let cx;
    let cy;
    if (target) {
      cx = +target.x || 0;
      cy = +target.y || 0;
    } else if (kids.length) {
      let sx = 0;
      let sy = 0;
      for (let i = 0; i < kids.length; i++) {
        sx += +kids[i].x || 0;
        sy += +kids[i].y || 0;
      }
      cx = sx / kids.length;
      cy = sy / kids.length;
    } else {
      return null;
    }
    const local = 36;
    let minx = Infinity;
    let maxx = -Infinity;
    let miny = Infinity;
    let maxy = -Infinity;
    for (let i = 0; i < kids.length; i++) {
      const x = +kids[i].x || 0;
      const y = +kids[i].y || 0;
      if (Math.abs(x - cx) <= local && Math.abs(y - cy) <= local) {
        if (x < minx) minx = x;
        if (x > maxx) maxx = x;
        if (y < miny) miny = y;
        if (y > maxy) maxy = y;
      }
    }
    if (!isFinite(minx)) {
      minx = cx - 18;
      maxx = cx + 18;
      miny = cy - 18;
      maxy = cy + 18;
    }
    const bw = Math.max(30, maxx - minx);
    const bh = Math.max(30, maxy - miny);
    return cnsCamFromSpan(view, c, cx, cy, bw, bh, 0.8, 8, 80, 2.5);
  }

  function cnsSessionGone(e) {
    const msg = errText(e);
    return /CNS session missing/i.test(msg) || !!(e && e.status === 400 && /session missing/i.test(msg));
  }

  async function reopenCnsSession(view) {
    const path = view && view.cns && view.cns.sdPath;
    if (!path) return false;
    startCnsImportOverlay(view);
    try {
      const data = await api.post("/api/cns/open", { path: path });
      if (!data || data.ok === false || data.session == null) return false;
      view.cns.session = data.session;
      return true;
    } catch (_) {
      return false;
    }
  }

  function cnsStarsQuery(view, box, focus, jump) {
    const pad = 80;
    return (
      "/api/cns/stars?session=" +
      encodeURIComponent(view.cns.session) +
      "&xmin=" +
      encodeURIComponent(box.xmin - pad) +
      "&xmax=" +
      encodeURIComponent(box.xmax + pad) +
      "&ymin=" +
      encodeURIComponent(box.ymin - pad) +
      "&ymax=" +
      encodeURIComponent(box.ymax + pad) +
      "&z=" +
      encodeURIComponent(box.scale != null ? box.scale : 1) +
      (focus ? "&focus=" + encodeURIComponent(focus) : "") +
      (jump ? "&jump=" + encodeURIComponent(jump) : "")
    );
  }

  async function loadCnsStars(view, opts) {
    opts = opts || {};
    const cns = view.cns;
    if (!cns) return;
    view.cnsStarsGen = (view.cnsStarsGen || 0) + 1;
    const gen = view.cnsStarsGen;
    const c0 = await ensureEngine(view);
    if (gen !== view.cnsStarsGen) return;
    view.cosmos = c0;
    if (!cns.session) {
      startCnsImportOverlay(view);
      if (c0 && c0.setScene) {
        c0.setScene({
          loading: true,
          children: [],
          loadText: cnsImportMessage(view),
          loadClass: "vload cns-load",
        });
      }
      return;
    }
    if (opts.leaveNeighborhood) cns.stack = [];
    const focus = cns.stack && cns.stack.length ? cns.stack[cns.stack.length - 1] : "";
    const jump = opts.jump || (opts.resetCamera ? focus : "");
    let box = { xmin: 0, xmax: 0, ymin: 0, ymax: 0, scale: 1 };
    if (opts.box) {
      box = opts.box;
    } else if (opts.leaveNeighborhood || (opts.resetCamera && !focus && !jump)) {
      box = { xmin: -1e7, xmax: 1e7, ymin: -1e7, ymax: 1e7, scale: 1 };
    } else if (c0 && c0.worldBBox && !opts.resetCamera) {
      try {
        box = c0.worldBBox() || box;
      } catch (_) {}
    }
    if (jump) box.scale = Math.max(1, box.scale != null ? box.scale : 1);
    let kids = [];
    let error = "";
    try {
      const data = await api.get(cnsStarsQuery(view, box, focus, jump));
      if (gen !== view.cnsStarsGen) return;
      kids = (data && data.stars) || [];
    } catch (e) {
      if (gen !== view.cnsStarsGen) return;
      if (cnsSessionGone(e) && (await reopenCnsSession(view))) {
        if (gen !== view.cnsStarsGen) return;
        try {
          const data = await api.get(cnsStarsQuery(view, box, focus, jump));
          if (gen !== view.cnsStarsGen) return;
          kids = (data && data.stars) || [];
        } catch (e2) {
          error = errText(e2);
        }
      } else {
        error = errText(e);
      }
    }
    if (gen !== view.cnsStarsGen) return;
    stopCnsImportOverlay(view);
    const c = await ensureEngine(view);
    view.cosmos = c;
    const liveCam = c && c.getCamera ? c.getCamera() : view.camera;
    if (c && c.setScene) {
      c.setScene({
        dirPath: "cns:" + (focus || "plane"),
        children: kids,
        hostLayout: true,
        ephemeral: true,
        camera: jump || opts.resetCamera ? undefined : liveCam,
        resetCamera: !!(opts.resetCamera && !jump),
        error: error,
      });
    }
    if (opts.resetCamera && !focus && !jump && c && c.setCamera) {
      frameCnsCloud(view, c, kids);
    }
    if (jump && c && c.flyTo) {
      const cam = cnsJumpCam(view, c, kids, jump);
      if (cam) c.flyTo(cam.x, cam.y, cam.scale, cam.r);
    }
    view.itemCount = kids.length;
    if (opts.resetCamera || opts.crumbs) paintCrumbs(view);
    paintStatus();
  }

  function cnsImportOverlayUp(view) {
    return !!(view && (view.cnsImportTimer || view.cnsImportAt));
  }

  async function retryInflightCnsStars(view) {
    if (!view || !view.cns || !cnsImportOverlayUp(view) || view.cnsResumeBusy) return;
    view.cnsResumeBusy = true;
    try {
      if (view.cns.session) {
        await loadCnsStars(view);
      } else if (view.cns.sdPath) {
        if (await reopenCnsSession(view)) await loadCnsStars(view);
      }
    } catch (_) {
    } finally {
      view.cnsResumeBusy = false;
    }
  }

  function pageIsVisible() {
    return !document.visibilityState || document.visibilityState === "visible";
  }

  function resumeAfterOcclusion() {
    if (!pageIsVisible()) return;
    resizeViews();
    (state.views || []).forEach((v) => {
      if (!v || v.minimized) return;
      const live = cosmosOf(v);
      if (live) {
        if (live.resize) live.resize();
        if (live.setLive) live.setLive(true);
      }
      retryInflightCnsStars(v);
    });
  }

  let resumeFromHostTimer = 0;
  function resumeFromHost() {
    if (!pageIsVisible()) return;
    if (resumeFromHostTimer) clearTimeout(resumeFromHostTimer);
    resumeFromHostTimer = setTimeout(() => {
      resumeFromHostTimer = 0;
      resumeAfterOcclusion();
    }, 40);
  }

  function paintCnsCrumbs(view) {
    const el = view.rootEl.querySelector(".crumbs");
    if (!el) return;
    el.innerHTML = "";
    el.classList.remove("goto");
    el.classList.add("cns-crumbs");
    const go = document.createElement("input");
    go.className = "cns-find";
    go.type = "search";
    go.spellcheck = false;
    go.placeholder = "Search Word / Enc / Post / Cns";
    go.title = "Find a word; Enter jumps";
    go.setAttribute("autocomplete", "off");
    bindCnsFind(view, go);
    el.appendChild(go);
    el.addEventListener("pointerdown", (e) => {
      if (e.target.closest("button, input")) return;
      go.focus();
    });
    paintPinStars();
  }

  function bindCnsFind(view, go) {
    go.addEventListener("pointerdown", (e) => e.stopPropagation());
    go.addEventListener("focus", () => {
      focusView(view);
      searchAnchor = go;
      syncSearchPlaceholder();
      const drop = $("#search-drop");
      if (go.value.trim() && drop && drop.innerHTML) {
        drop.hidden = false;
        placeSearchDrop();
      }
    });
    go.addEventListener("input", () => {
      focusView(view);
      searchAnchor = go;
      clearTimeout(searchTimer);
      searchTimer = setTimeout(() => doSearch(go.value.trim()), 200);
    });
    go.addEventListener("keydown", (e) => {
      const drop = $("#search-drop");
      const hits = drop ? $$(".search-hit", drop) : [];
      const on = hits.findIndex((h) => h.classList.contains("active"));
      if (e.key === "ArrowDown") {
        e.preventDefault();
        const n = hits[Math.min(hits.length - 1, on + 1)] || hits[0];
        hits.forEach((h) => h.classList.remove("active"));
        if (n) n.classList.add("active");
      } else if (e.key === "ArrowUp") {
        e.preventDefault();
        const n = hits[Math.max(0, on - 1)];
        hits.forEach((h) => h.classList.remove("active"));
        if (n) n.classList.add("active");
      } else if (e.key === "Enter") {
        const hit = hits.find((h) => h.classList.contains("active")) || hits[0];
        if (!hit) return;
        e.preventDefault();
        activateHit(JSON.parse(hit.dataset.hit), e.altKey);
      } else if (e.key === "Escape") {
        e.preventDefault();
        const had = !!(go.value || "").trim();
        go.value = "";
        const dropEl = $("#search-drop");
        if (dropEl) {
          dropEl.hidden = true;
          dropEl.innerHTML = "";
        }
        if (!had && cnsInNeighborhood(view)) {
          cnsLeaveNeighborhood(view);
          go.blur();
        }
      }
    });
  }

  function openCnsStar(view, star) {
    if (!star || !view.cns) return;
    if (star.role === "cluster") {
      const c = cosmosOf(view);
      if (c && c.flyTo) c.flyTo(star.x, star.y, Math.max(0.7, ((c.getCamera && c.getCamera().scale) || 0.35) * 2.4));
      loadCnsStars(view);
      return;
    }
    const ch = star.name;
    if (!view.cns.stack) view.cns.stack = [];
    if (view.cns.stack[view.cns.stack.length - 1] !== ch) view.cns.stack.push(ch);
    loadCnsStars(view, { jump: ch });
    showCnsDetail(view, ch, false);
  }

  function cnsInNeighborhood(view) {
    return !!(view && view.cns && view.cns.stack && view.cns.stack.length);
  }

  function cnsLeaveNeighborhood(view, opts) {
    if (!cnsInNeighborhood(view)) return;
    const jump = opts && opts.jump;
    return loadCnsStars(view, {
      leaveNeighborhood: true,
      jump: jump || undefined,
      resetCamera: !jump,
      box: opts && opts.box,
    });
  }

  function cnsLeave(view) {
    if (!view) return;
    if (cnsInNeighborhood(view)) {
      cnsLeaveNeighborhood(view);
      return;
    }
    if (view.cns) {
      const sd = view.cns.sdPath;
      stopCnsImportOverlay(view);
      view.cns = null;
      const panel = view.rootEl && view.rootEl.querySelector(".cns-inspect");
      if (panel) {
        panel.hidden = true;
        panel.innerHTML = "";
      }
      const up = parentPath(sd);
      if (up && up !== sd) navigateView(view, up, true);
      return;
    }
    const up = parentPath(view.path);
    if (up && up !== view.path) navigateView(view, up, true);
  }

  function showCnsDetail(view, ch, hover) {
    if (!view || !view.cns || !ch) return;
    const panel = view.rootEl && view.rootEl.querySelector(".cns-inspect");
    if (!panel) return;
    if (hover && view.cnsQueryChar === ch && panel.dataset.ready === "1") return;
    clearTimeout(view.cnsQueryTimer);
    view.cnsQueryTimer = setTimeout(async () => {
      view.cnsQueryChar = ch;
      panel.hidden = false;
      panel.dataset.ready = "0";
      panel.innerHTML = "<div class='k'>Word</div><div class='v'>" + String(ch).replace(/</g, "") + "</div>";
      try {
        let data;
        try {
          data = await api.get(
            "/api/cns/query?session=" +
              encodeURIComponent(view.cns.session) +
              "&char=" +
              encodeURIComponent(ch)
          );
        } catch (e) {
          if (!(cnsSessionGone(e) && (await reopenCnsSession(view)))) throw e;
          data = await api.get(
            "/api/cns/query?session=" +
              encodeURIComponent(view.cns.session) +
              "&char=" +
              encodeURIComponent(ch)
          );
        }
        if (view.cnsQueryChar !== ch) return;
        const d = (data && data.detail) || {};
        const rows = [
          ["Word", d.char || ch],
          ["Cns", d.cns],
          ["Ids", d.ids],
          ["SequenceLevel", d.hasSeqLevel ? String(d.seqLevel) : ""],
          ["Enc", d.encoding],
          ["Post", d.post],
          ["stroke_order", data.strokeOrder],
          ["components_of", data.componentsOf],
        ];
        panel.innerHTML = rows
          .filter((r) => r[1] != null && String(r[1]).length)
          .map(
            (r) =>
              "<div class='k'>" +
              r[0] +
              "</div><div class='v'>" +
              String(r[1]).replace(/</g, "") +
              "</div>"
          )
          .join("");
        panel.dataset.ready = "1";
      } catch (e) {
        panel.innerHTML =
          "<div class='k'>Word</div><div class='v'>" +
          String(ch).replace(/</g, "") +
          "</div><div class='v'>" +
          errText(e).replace(/</g, "") +
          "</div>";
      }
    }, hover ? 120 : 0);
  }

  async function runCargoTest() {
    const v = focusedView();
    const cwd = (v && v.root) || state.meta.defaultRoot || "";
    const floats = $("#floats");
    const el = document.createElement("div");
    el.className = "float-win focused";
    el.style.left = "72px";
    el.style.top = "56px";
    el.style.width = "640px";
    el.style.height = "360px";
    el.innerHTML =
      '<div class="float-title"><span class="t">cargo test</span><button type="button" data-act="pin" title="Pin">☆</button><button type="button" data-act="x">×</button></div>' +
      '<div class="float-body"><pre class="test-out">Running cargo test…\n</pre></div><div class="float-resize"></div>';
    floats.appendChild(el);
    const out = el.querySelector(".test-out");
    el.querySelector('[data-act="x"]').onclick = () => el.remove();
    const pinBtn = el.querySelector('[data-act="pin"]');
    function paintTestPin() {
      const on = !!permPinFor("test", "cargo-test");
      pinBtn.textContent = on ? "★" : "☆";
      pinBtn.title = on ? "Unpin" : "Pin";
    }
    pinBtn.onclick = () => {
      togglePermPin({ kind: "test", path: "cargo-test", title: "cargo test" });
      paintTestPin();
    };
    paintTestPin();
    wireFloat(el);
    let secs = 0;
    const tick = setInterval(() => {
      secs += 1;
      if (out && !out.dataset.done) out.textContent = "Running cargo test… " + secs + "s\n";
    }, 1000);
    try {
      const data = await api.post("/api/cargo-test", { cwd });
      out.dataset.done = "1";
      out.textContent =
        (data && (data.output || data.stdout || data.raw)) || JSON.stringify(data, null, 2);
      if (data && data.stderr) out.textContent += (out.textContent ? "\n" : "") + data.stderr;
    } catch (e) {
      out.dataset.done = "1";
      out.textContent = "cargo test failed: " + errText(e);
    } finally {
      clearInterval(tick);
    }
  }

  async function refreshWeb() {
    try {
      await flushSave();
    } catch (_) {}
    const url = new URL(location.href);
    url.searchParams.set("web", String(Date.now()));
    location.replace(url.href);
  }

  async function runRebuildUi() {
    const floats = $("#floats");
    const el = document.createElement("div");
    el.className = "float-win focused";
    el.style.left = "72px";
    el.style.top = "56px";
    el.style.width = "640px";
    el.style.height = "360px";
    el.innerHTML =
      '<div class="float-title"><span class="t">Rebuild StarDust UI</span><button type="button" data-act="pin" title="Pin">☆</button><button type="button" data-act="x">×</button></div>' +
      '<div class="float-body"><pre class="test-out">cargo build -p fs-viz\nthen target\\debug\\fs-viz.exe --desktop --replace\n\nBuilding…\n</pre></div><div class="float-resize"></div>';
    floats.appendChild(el);
    const out = el.querySelector(".test-out");
    el.querySelector('[data-act="x"]').onclick = () => el.remove();
    const pinBtn = el.querySelector('[data-act="pin"]');
    function paintRebuildPin() {
      const on = !!permPinFor("rebuild", "rebuild-ui");
      pinBtn.textContent = on ? "★" : "☆";
      pinBtn.title = on ? "Unpin" : "Pin";
    }
    pinBtn.onclick = () => {
      togglePermPin({ kind: "rebuild", path: "rebuild-ui", title: "Rebuild StarDust UI" });
      paintRebuildPin();
    };
    paintRebuildPin();
    wireFloat(el);
    let secs = 0;
    const tick = setInterval(() => {
      secs += 1;
      if (out && !out.dataset.done) {
        out.textContent =
          "cargo build -p fs-viz\nthen target\\debug\\fs-viz.exe --desktop --replace\n\nBuilding… " +
          secs +
          "s\n";
      }
    }, 1000);
    try {
      try {
        await flushSave();
      } catch (_) {}
      const data = await api.post("/api/rebuild-ui", {});
      out.dataset.done = "1";
      out.textContent =
        (data && (data.output || data.stdout || data.raw)) || JSON.stringify(data, null, 2);
      if (data && data.stderr) out.textContent += (out.textContent ? "\n" : "") + data.stderr;
    } catch (e) {
      out.dataset.done = "1";
      out.textContent = "Rebuild failed: " + errText(e);
    } finally {
      clearInterval(tick);
    }
  }

  async function openInCursor() {
    try {
      await api.post("/api/open-cursor", {});
    } catch (e) {
      toast(errText(e));
    }
  }

  async function openFolder(view) {
    const v = view || focusedView();
    let pickedName = "";
    if (window.showDirectoryPicker) {
      try {
        const handle = await window.showDirectoryPicker();
        pickedName = (handle && handle.name) || "";
      } catch (e) {
        if (e && e.name === "AbortError") return;
      }
    }
    const start = (v && v.root) || state.meta.defaultRoot || "";
    let prefill = start;
    if (pickedName && start) {
      const parent = parentPath(start);
      if (parent) prefill = joinPath(parent, pickedName);
    }
    $("#modal-title").textContent = "Open folder";
    $("#modal-body").innerHTML =
      (pickedName ? "<p>Picked folder name: <code>" + pickedName.replace(/</g, "") + "</code>. Browsers do not expose the real path — enter the absolute path.</p>" : "<p>Enter an absolute folder path.</p>") +
      '<input id="folder-path"/>';
    $("#modal-actions").innerHTML = "";
    const cancel = document.createElement("button");
    cancel.type = "button";
    cancel.textContent = "Cancel";
    const ok = document.createElement("button");
    ok.type = "button";
    ok.className = "primary";
    ok.textContent = "Open";
    $("#modal-actions").append(cancel, ok);
    $("#modal").hidden = false;
    const inp = $("#folder-path");
    inp.value = prefill;
    inp.focus();
    inp.select();
    return new Promise((resolve) => {
      cancel.onclick = () => {
        $("#modal").hidden = true;
        resolve(null);
      };
      ok.onclick = () => {
        const p = inp.value.trim();
        $("#modal").hidden = true;
        if (p && v) {
          v.root = p;
          navigateView(v, p, true);
        }
        resolve(p);
      };
    });
  }

  function arrangeAll(key) {
    state.sortMode = key;
    state.views.forEach((v) => {
      if (v.cns) return;
      const c = cosmosOf(v);
      if (!c) return;
      if (key === "galaxy") c.galaxy();
      else c.arrangeBy(key);
    });
    scheduleSave();
  }

  function refreshModeButton() {
    const el = $("#btn-mode");
    if (!el) return;
    const is3d = viewMode(focusedView()) === "3d";
    const accel = el.querySelector(".accel");
    el.textContent = is3d ? "3D" : "2D";
    if (accel) el.appendChild(accel);
  }

  async function setMode(mode, view) {
    view = view || focusedView();
    if (!view) return;
    const next = mode === "3d" ? "3d" : "2d";
    if (view.cosmos) view.camera = view.cosmos.getCamera();
    view.mode = next;
    state.mode = next;
    refreshModeButton();
    await loadChildren(view);
    scheduleSave();
  }

  function resizeViews() {
    state.views.forEach((v) => {
      if (v.cosmos2) v.cosmos2.resize();
      if (v.cosmos3) v.cosmos3.resize();
    });
  }

  function bindMenus() {
    $$(".menu-btn").forEach((btn) => {
      btn.addEventListener("click", (e) => {
        e.stopPropagation();
        const id = "menu-" + btn.dataset.menu;
        const pop = document.getElementById(id);
        const open = pop.hidden;
        hidePop();
        if (open) {
          pop.hidden = false;
          btn.classList.add("open");
          if (btn.dataset.menu === "edit") refreshEditMenu();
          if (btn.dataset.menu === "view") {
            refreshViewMenu();
            refreshModeButton();
          }
        }
      });
    });
    document.addEventListener("click", (e) => {
      const cmdBtn = e.target.closest(".menu-pop button[data-cmd]");
      if (cmdBtn && cmdBtn.dataset.cmd) {
        hidePop();
        runCmd(cmdBtn.dataset.cmd);
        e.stopPropagation();
        return;
      }
      if (!e.target.closest(".menu") && !e.target.closest("#ctx") && !e.target.closest("#search") && !e.target.closest("#search-drop")) {
        hidePop();
      }
    });
  }

  function runCmd(cmd) {
    const v = focusedView();
    const sel = selection();
    switch (cmd) {
      case "open-folder":
        openFolder(v);
        break;
      case "open-desktop":
        if (state.meta.desktopPath) navigateView(v, state.meta.desktopPath, true);
        break;
      case "open-this-pc":
        navigateView(v, THIS_PC, true);
        break;
      case "ui-stardust":
        setUiMode("stardust");
        break;
      case "ui-windows":
        setUiMode("windows");
        break;
      case "toggle-ui-mode":
        setUiMode(state.uiMode === "stardust" ? "windows" : "stardust");
        break;
      case "cut":
        if (sel.length) clip("cut", sel);
        break;
      case "copy":
        if (sel.length) clip("copy", sel);
        break;
      case "paste":
        pasteInto(v);
        break;
      case "rename":
        if (sel[0]) startRename(v, sel[0]);
        break;
      case "send-agent":
        attachStars(sel);
        break;
      case "tile":
        SD.editor.tile();
        break;
      case "cascade":
        SD.editor.cascade();
        break;
      case "sort-name":
        arrangeAll("name");
        break;
      case "sort-type":
        arrangeAll("type");
        break;
      case "sort-size":
        arrangeAll("size");
        break;
      case "galaxy":
        arrangeAll("galaxy");
        break;
      case "about":
        modal({
          title: "About StarDust",
          html: "<p>StarDust cosmos is a daily-dev shell over the host file tree. Directories and files are stars; dumpable IR is the source of truth for Inspect, and <code>.sd</code> is a copy.</p>",
          buttons: [{ label: "OK", primary: true, value: true }],
        });
        break;
      case "toggle-mode":
        setMode(viewMode(v) === "2d" ? "3d" : "2d", v);
        break;
      case "cargo-test":
        runCargoTest();
        break;
      case "refresh-web":
        refreshWeb();
        break;
      case "rebuild-ui":
        runRebuildUi();
        break;
      case "open-cursor":
        openInCursor();
        break;
      case "select-all":
        selectAllStars(v);
        break;
      case "new-folder":
        if (v) newEntry(v, "dir");
        break;
      case "new-split":
        splitFocused(v);
        break;
      case "close-view":
        closeView(v);
        break;
      case "refresh":
        if (v) loadChildren(v);
        break;
      case "mode-2d":
        setMode("2d", v);
        break;
      case "mode-3d":
        setMode("3d", v);
        break;
      case "keyboard-shortcuts":
        setShortcutOpen(true);
        break;
    }
  }

  let searchTimer = null;
  let searchSeq = 0;
  let searchAnchor = null;
  function syncSearchPlaceholder() {
    const inp = $("#search-input");
    if (!inp) return;
    const fv = focusedView();
    inp.placeholder =
      fv && fv.cns ? "Search Word / Enc / Post / Cns" : "Search files and folders";
  }
  function placeSearchDrop() {
    const drop = $("#search-drop");
    const inp = searchAnchor || $("#search-input");
    if (!inp || !drop || drop.hidden) return;
    const r = inp.getBoundingClientRect();
    const width = Math.min(560, Math.max(r.width, 360), window.innerWidth - 24);
    const left = Math.max(8, Math.min(r.left, window.innerWidth - width - 8));
    const top = r.bottom + 6;
    const maxH = Math.max(160, Math.min(window.innerHeight - top - 16, Math.floor(window.innerHeight * 0.6), 480));
    drop.style.left = left + "px";
    drop.style.top = top + "px";
    drop.style.width = width + "px";
    drop.style.maxHeight = maxH + "px";
  }

  function bindSearch() {
    const inp = $("#search-input");
    const drop = $("#search-drop");
    drop.addEventListener("pointerdown", (e) => e.stopPropagation());
    window.addEventListener("resize", placeSearchDrop);
    inp.addEventListener("focus", () => {
      searchAnchor = inp;
      syncSearchPlaceholder();
      if (inp.value.trim() && drop.innerHTML) {
        drop.hidden = false;
        placeSearchDrop();
      }
    });
    inp.addEventListener("input", () => {
      clearTimeout(searchTimer);
      searchTimer = setTimeout(() => doSearch(inp.value.trim()), 200);
    });
    inp.addEventListener("keydown", (e) => {
      const hits = $$(".search-hit", drop);
      const on = hits.findIndex((h) => h.classList.contains("active"));
      if (e.key === "ArrowDown") {
        e.preventDefault();
        const n = hits[Math.min(hits.length - 1, on + 1)] || hits[0];
        hits.forEach((h) => h.classList.remove("active"));
        if (n) n.classList.add("active");
      } else if (e.key === "ArrowUp") {
        e.preventDefault();
        const n = hits[Math.max(0, on - 1)];
        hits.forEach((h) => h.classList.remove("active"));
        if (n) n.classList.add("active");
      } else if (e.key === "Enter") {
        const hit = hits.find((h) => h.classList.contains("active")) || hits[0];
        if (!hit) return;
        e.preventDefault();
        activateHit(JSON.parse(hit.dataset.hit), e.altKey);
      } else if (e.key === "Escape") drop.hidden = true;
    });
  }

  function searchRootList() {
    const out = [];
    const seen = new Set();
    function add(r) {
      if (!r) return;
      const k = pathKey(r);
      if (!k || seen.has(k)) return;
      seen.add(k);
      out.push(r);
    }
    const fv = focusedView();
    if (fv && fv.path && fv.path !== THIS_PC) add(fv.path);
    add((fv && fv.root) || (state.meta && state.meta.defaultRoot) || "");
    state.views.forEach((v) => add(v.root));
    return out;
  }

  function cnsSearchTarget() {
    const fv = focusedView();
    if (fv && fv.cns && !fv.minimized) return fv;
    return null;
  }

  async function doSearch(q) {
    const drop = $("#search-drop");
    const seq = ++searchSeq;
    if (!q) {
      drop.hidden = true;
      drop.innerHTML = "";
      return;
    }
    drop.hidden = false;
    drop.innerHTML = '<div class="search-hit muted">Searching…</div>';
    placeSearchDrop();
    const fvCns = cnsSearchTarget();
    if (fvCns && fvCns.cns) {
      if (!fvCns.cns.session) {
        drop.innerHTML = '<div class="search-hit muted">Loading Word…</div>';
        placeSearchDrop();
        return;
      }
      try {
        let data;
        try {
          data = await api.get(
            "/api/cns/search?session=" +
              encodeURIComponent(fvCns.cns.session) +
              "&q=" +
              encodeURIComponent(q)
          );
        } catch (e) {
          if (!(cnsSessionGone(e) && (await reopenCnsSession(fvCns)))) throw e;
          data = await api.get(
            "/api/cns/search?session=" +
              encodeURIComponent(fvCns.cns.session) +
              "&q=" +
              encodeURIComponent(q)
          );
        }
        if (seq !== searchSeq) return;
        paintSearchHits(drop, (data && data.hits) || [], q, false);
        placeSearchDrop();
      } catch (e) {
        if (seq !== searchSeq) return;
        paintSearchHits(drop, [], q, false);
      }
      return;
    }
    const roots = searchRootList();
    let hits = [];
    try {
      const seen = new Set();
      for (let i = 0; i < roots.length; i++) {
        if (seq !== searchSeq) return;
        const root = roots[i];
        let list = [];
        try {
          const data = await api.get(
            "/api/search?root=" + encodeURIComponent(root) + "&q=" + encodeURIComponent(q)
          );
          list = Array.isArray(data) ? data : (data && (data.hits || data.results)) || [];
        } catch (_) {
          continue;
        }
        list.forEach((h) => {
          const k = pathKey(h.path || "");
          if (!k || seen.has(k)) return;
          seen.add(k);
          hits.push(h);
        });
        if (i === 0) {
          paintSearchHits(drop, hits, q, true);
          placeSearchDrop();
        }
      }
      if (!roots.length) throw new Error("no roots");
    } catch (_) {
      if (seq !== searchSeq) return;
      const v = focusedView();
      const c = cosmosOf(v);
      const stars = c && c.stars ? c.stars() : [];
      hits = stars
        .filter((s) => s.name.toLowerCase().includes(q.toLowerCase()))
        .map((s) => ({ name: s.name, path: s.path, kind: s.kind, parent: v.path }));
    }
    if (seq !== searchSeq) return;
    paintSearchHits(drop, hits, q, false);
    placeSearchDrop();
  }

  function paintSearchHits(drop, hits, q, partial) {
    drop.innerHTML = "";
    const list = hits.slice(0, 40);
    if (!list.length) {
      const empty = document.createElement("div");
      empty.className = "search-hit muted";
      empty.textContent = partial ? "Searching…" : "No matches for “" + q + "”";
      drop.appendChild(empty);
      drop.hidden = false;
      return;
    }
    list.forEach((h) => {
      const parent = h.parent || h.parentPath || parentPath(h.path);
      const b = document.createElement("button");
      b.type = "button";
      b.className = "search-hit";
      b.dataset.hit = JSON.stringify(Object.assign({}, h, { parent }));
      b.innerHTML =
        "<div>" +
        String(h.name || baseName(h.path)).replace(/</g, "") +
        '</div><div class="p">' +
        String(
          h.kind === "cns"
            ? [h.cns, h.encoding, h.post].filter(Boolean).join(" · ")
            : h.path || ""
        ).replace(/</g, "") +
        "</div>";
      b.onmouseenter = () => {
        const fv = focusedView();
        const c = cosmosOf(fv);
        if (!c) return;
        if (fv.path === parent) c.highlight(h.name || baseName(h.path));
        else if (c.highlightParent) c.highlightParent(parent);
      };
      b.onclick = (ev) => activateHit(Object.assign({}, h, { parent }), ev.altKey);
      drop.appendChild(b);
    });
    if (partial) {
      const more = document.createElement("div");
      more.className = "search-hit muted";
      more.textContent = "Searching more folders…";
      drop.appendChild(more);
    }
    drop.hidden = false;
  }

  function clearSearch() {
    const inp = $("#search-input");
    const drop = $("#search-drop");
    searchSeq += 1;
    clearTimeout(searchTimer);
    if (inp) inp.value = "";
    $$(".cns-find").forEach((el) => {
      el.value = "";
    });
    searchAnchor = null;
    if (drop) {
      drop.innerHTML = "";
      drop.hidden = true;
    }
  }

  async function activateHit(h, jump) {
    clearSearch();
    const cnsView = cnsSearchTarget();
    if (cnsView && cnsView.cns && (h.kind === "cns" || String(h.path || "").indexOf("cns:") === 0)) {
      focusView(cnsView);
      if (cnsInNeighborhood(cnsView)) {
        const hx = +h.x;
        const hy = +h.y;
        const box =
          Number.isFinite(hx) && Number.isFinite(hy)
            ? { xmin: hx - 800, xmax: hx + 800, ymin: hy - 800, ymax: hy + 800, scale: 1 }
            : undefined;
        await cnsLeaveNeighborhood(cnsView, { jump: h.name, box: box });
      } else {
        await loadCnsStars(cnsView, { jump: h.name });
      }
      const c = cosmosOf(cnsView);
      if (c) {
        if (c.highlight) c.highlight(h.name);
        if (c.selectNames) c.selectNames([h.name]);
      }
      showCnsDetail(cnsView, h.name, false);
      return;
    }
    const v = focusedView();
    const parent = h.parent || parentPath(h.path);
    if (jump) {
      await navigateView(v, parent, true);
      const c = cosmosOf(v);
      if (c) {
        c.highlight(h.name || baseName(h.path));
        c.selectNames([h.name || baseName(h.path)]);
      }
      return;
    }
    if (h.kind === "dir") navigateView(v, h.path, true);
    else SD.editor.open(h.path);
  }

  function selectAllStars(view) {
    view = view || focusedView();
    const c = cosmosOf(view);
    if (!c || !c.selectNames || !c.stars) return;
    c.selectNames(c.stars().map((s) => s.name));
  }

  function focusPathField(view) {
    view = view || focusedView();
    if (view && view.cns) {
      const find = view.rootEl && view.rootEl.querySelector(".cns-find");
      if (find) {
        focusView(view);
        searchAnchor = find;
        find.focus();
        if (typeof find.select === "function") find.select();
      }
      return;
    }
    const el = view && view.rootEl && view.rootEl.querySelector(".crumbs");
    const inp = el && el.querySelector(".crumb-path");
    if (!inp) return;
    inp.value = view.path === THIS_PC ? "This PC" : view.path;
    el.classList.add("goto");
    inp.focus();
    if (typeof inp.select === "function") inp.select();
  }

  function goParent(view) {
    view = view || focusedView();
    if (!view) return;
    if (view.cns) {
      cnsLeave(view);
      return;
    }
    if (view.path === THIS_PC) return;
    const up = parentPath(view.path);
    if (up && up !== view.path) navigateView(view, up, true);
    else navigateView(view, THIS_PC, true);
  }

  function shortcutOverlayEl() {
    return $("#shortcut-overlay");
  }

  function isShortcutOpen() {
    const el = shortcutOverlayEl();
    return !!(el && !el.hidden);
  }

  function setShortcutOpen(open) {
    const el = shortcutOverlayEl();
    if (!el) return;
    el.hidden = !open;
  }

  function toggleShortcutOverlay() {
    setShortcutOpen(!isShortcutOpen());
  }

  function eatKey(e) {
    e.preventDefault();
    if (typeof e.stopImmediatePropagation === "function") e.stopImmediatePropagation();
    else e.stopPropagation();
  }

  function letterKey(e, letter, code) {
    return (e.key || "").toLowerCase() === letter || e.code === code;
  }

  function bindKeys() {
    const overlay = shortcutOverlayEl();
    if (overlay) {
      overlay.addEventListener("click", (e) => {
        if (e.target === overlay) setShortcutOpen(false);
      });
    }
    // Capture so we beat Monaco and Edge accelerators. Agent chords run even while typing
    // (search / path / editor / composer), matching Cursor’s Ctrl+I toggle.
    window.addEventListener(
      "keydown",
      (e) => {
        const typing = !!(SD.editor && SD.editor.isEditing && SD.editor.isEditing());
        const renameEl = $("#rename-overlay");
        const inRename = !!(renameEl && !renameEl.hidden);
        const inComposer = !!(e.target && e.target.id === "agent-input");
        const inField =
          e.target &&
          (e.target.isContentEditable ||
            /^(INPUT|TEXTAREA|SELECT)$/.test(e.target.tagName || ""));
        const mod = e.ctrlKey || e.metaKey;
        const overlayOpen = isShortcutOpen();
        const cosmosOk = !overlayOpen && !inRename && !inField && !typing;

        if (e.key === "F1" || (mod && !e.altKey && !e.shiftKey && (e.key === "/" || e.code === "Slash"))) {
          eatKey(e);
          toggleShortcutOverlay();
          return;
        }
        if (e.key === "Escape") {
          if (overlayOpen) {
            setShortcutOpen(false);
            return;
          }
          hidePop();
          const drop = $("#search-drop");
          const dropOpen = drop && !drop.hidden;
          if (drop) drop.hidden = true;
          if (inField) return;
          if (dropOpen) return;
          const fvEsc = focusedView();
          if (cnsInNeighborhood(fvEsc)) {
            eatKey(e);
            cnsLeaveNeighborhood(fvEsc);
          }
          return;
        }
        if (mod && !e.altKey && letterKey(e, "s", "KeyS")) {
          eatKey(e);
          if (SD.editor) SD.editor.saveFocused();
          return;
        }

        if (mod && !e.altKey && letterKey(e, "i", "KeyI")) {
          eatKey(e);
          if (!SD.agent) return;
          if (e.shiftKey) {
            SD.agent.focusComposer();
          } else if (state.agentOpen) {
            SD.agent.setOpen(false);
          } else {
            SD.agent.setOpen(true);
            SD.agent.focusComposer();
          }
          return;
        }
        if (mod && e.shiftKey && !e.altKey && letterKey(e, "a", "KeyA")) {
          eatKey(e);
          runCmd("send-agent");
          return;
        }
        if (mod && e.shiftKey && !e.altKey && letterKey(e, "t", "KeyT")) {
          eatKey(e);
          if (SD.agent && SD.agent.newThread) SD.agent.newThread();
          return;
        }
        if (mod && e.shiftKey && !e.altKey && (e.key === "." || e.code === "Period")) {
          eatKey(e);
          if (SD.agent && SD.agent.stop) SD.agent.stop();
          return;
        }

        if (mod && !e.altKey && !e.shiftKey && e.key === "F5") {
          eatKey(e);
          runCmd("refresh-web");
          return;
        }
        if (e.key === "F5") {
          eatKey(e);
          if (cosmosOk) runCmd("refresh");
          return;
        }
        if (mod && !e.altKey && !e.shiftKey && letterKey(e, "a", "KeyA")) {
          if (inField || (typing && !inComposer)) return;
          eatKey(e);
          if (cosmosOk) runCmd("select-all");
          return;
        }
        if (mod && !e.altKey && !e.shiftKey && letterKey(e, "f", "KeyF")) {
          if (!cosmosOk) return;
          eatKey(e);
          const fv = focusedView();
          if (fv && fv.cns) {
            focusPathField(fv);
            return;
          }
          const inp = $("#search-input");
          if (inp) inp.focus();
          return;
        }
        if (mod && !e.altKey && !e.shiftKey && letterKey(e, "l", "KeyL")) {
          if (!cosmosOk) return;
          eatKey(e);
          focusPathField(focusedView());
          return;
        }
        if (mod && !e.altKey && !e.shiftKey && letterKey(e, "o", "KeyO")) {
          eatKey(e);
          if (cosmosOk) runCmd("open-folder");
          return;
        }
        if (mod && !e.altKey && !e.shiftKey && letterKey(e, "d", "KeyD")) {
          eatKey(e);
          if (cosmosOk) runCmd("open-desktop");
          return;
        }
        if (mod && !e.altKey && !e.shiftKey && letterKey(e, "n", "KeyN")) {
          eatKey(e);
          if (cosmosOk) runCmd("new-split");
          return;
        }
        if (mod && !e.altKey && !e.shiftKey && letterKey(e, "w", "KeyW")) {
          eatKey(e);
          if (cosmosOk) runCmd("close-view");
          return;
        }
        if (mod && e.shiftKey && !e.altKey && letterKey(e, "n", "KeyN")) {
          eatKey(e);
          if (cosmosOk) runCmd("new-folder");
          return;
        }
        if (mod && e.shiftKey && !e.altKey && letterKey(e, "p", "KeyP")) {
          eatKey(e);
          if (cosmosOk) runCmd("open-this-pc");
          return;
        }
        if (mod && e.shiftKey && !e.altKey && e.code === "Digit2") {
          eatKey(e);
          if (cosmosOk) runCmd("mode-2d");
          return;
        }
        if (mod && e.shiftKey && !e.altKey && e.code === "Digit3") {
          eatKey(e);
          if (cosmosOk) runCmd("mode-3d");
          return;
        }

        if (inRename) return;
        if (overlayOpen) return;
        if (inField && e.key !== "Escape") {
          const fvField = focusedView();
          if (!(e.altKey && e.key === "ArrowUp" && cnsInNeighborhood(fvField))) return;
        }
        if (typing) return;
        const vType = focusedView();
        if (
          vType &&
          vType.cns &&
          !mod &&
          !e.altKey &&
          e.key &&
          e.key.length === 1
        ) {
          const find = vType.rootEl && vType.rootEl.querySelector(".cns-find");
          if (find) {
            eatKey(e);
            searchAnchor = find;
            find.focus();
            find.value = (find.value || "") + e.key;
            clearTimeout(searchTimer);
            searchTimer = setTimeout(() => doSearch(find.value.trim()), 200);
            return;
          }
        }
        const v = focusedView();
        const star = targetStar(v);
        const sel = selection();
        if (e.altKey && e.key === "ArrowUp") {
          eatKey(e);
          goParent(v);
          return;
        }
        if (e.key === "Backspace" && !mod && !e.altKey) {
          eatKey(e);
          goParent(v);
          return;
        }
        if (e.altKey && e.key === "ArrowLeft") {
          eatKey(e);
          if (!v || !v.pathHist || !(v.pathIdx > 0)) return;
          v.pathIdx -= 1;
          navigateView(v, v.pathHist[v.pathIdx], true, true);
          return;
        }
        if (e.altKey && e.key === "ArrowRight") {
          eatKey(e);
          if (!v || !v.pathHist || v.pathIdx >= v.pathHist.length - 1) return;
          v.pathIdx += 1;
          navigateView(v, v.pathHist[v.pathIdx], true, true);
          return;
        }
        if (e.altKey && e.key === "Enter") {
          eatKey(e);
          if (star) showProps(star);
          return;
        }
        if (e.key === "Enter") {
          if (star) {
            const c = cosmosOf(v);
            if (star.kind === "dir" && c && c.flyEnter) c.flyEnter(star, () => openStar(v, star));
            else openStar(v, star);
          }
        } else if (e.key === "F2") {
          const tgt = e.target;
          if (tgt && tgt.closest && tgt.closest("#agent-tabs")) {
            eatKey(e);
            if (SD.agent && SD.agent.renameFocusedThread) SD.agent.renameFocusedThread();
            return;
          }
          if (tgt && tgt.closest && tgt.closest("#thread-list")) {
            eatKey(e);
            if (SD.agent && SD.agent.renameFocusedThread) SD.agent.renameFocusedThread();
            return;
          }
          if (tgt && tgt.closest && tgt.closest("#agent-thread-title")) {
            eatKey(e);
            if (SD.agent && SD.agent.renameFocusedThread) SD.agent.renameFocusedThread();
            return;
          }
          eatKey(e);
          if (star) startRename(v, star);
        } else if (e.key === "Delete") {
          if (sel.length) deleteStars(v, sel);
          else if (star) deleteStars(v, [star]);
        } else if (mod && letterKey(e, "c", "KeyC")) {
          if (sel.length) {
            eatKey(e);
            clip("copy", sel);
          }
        } else if (mod && letterKey(e, "x", "KeyX")) {
          if (sel.length) {
            eatKey(e);
            clip("cut", sel);
          }
        } else if (mod && letterKey(e, "v", "KeyV")) {
          eatKey(e);
          pasteInto(v);
        }
      },
      true
    );
  }

  function formatBytes(n) {
    if (n == null || n === "") return "";
    let x = Number(n);
    if (!isFinite(x) || x < 0) return "";
    const u = ["B", "KB", "MB", "GB", "TB"];
    let i = 0;
    while (x >= 1024 && i < u.length - 1) {
      x /= 1024;
      i++;
    }
    return (i === 0 ? String(Math.round(x)) : x.toFixed(1)) + " " + u[i];
  }

  function volumeFreeLabel(path) {
    if (!path || path === THIS_PC) {
      return state.meta.volumeFree ? formatBytes(state.meta.volumeFree) + " free" : "";
    }
    const m = /^([A-Za-z]:)/.exec(path);
    const drives = state.drives || [];
    const d = m && drives.find((x) => String(x.path || "").toUpperCase().startsWith(m[1].toUpperCase()));
    if (d && d.free != null) return formatBytes(d.free) + " free";
    if (state.meta.volumeFree) return formatBytes(state.meta.volumeFree) + " free";
    return "";
  }

  function paintStatus() {
    const dot = $("#status-health");
    const text = $("#status-text");
    if (!dot || !text) return;
    const h = (state.meta.health && state.meta.health.color) || "green";
    dot.className = "health-dot " + h;
    const v = focusedView();
    const bits = [];
    bits.push(state.uiMode === "stardust" ? "StarDust UI" : "Windows UI");
    if (state.meta.replaceArmed) bits.push("Explorer hooked");
    bits.push(selection().length + " selected");
    bits.push(((v && v.itemCount) || 0) + " items");
    const free = volumeFreeLabel(v && v.path);
    if (free) bits.push(free);
    if (state.meta.health && state.meta.health.reason) bits.push(state.meta.health.reason);
    bits.push("failsafe " + (state.meta.failsafeHotkey || "Ctrl+Shift+F12"));
    text.textContent = bits.join(" · ");
  }

  async function refreshHealth() {
    try {
      const h = await api.get("/api/ui-health");
      state.meta.health = h;
    } catch (_) {}
    try {
      const d = await api.get("/api/drives");
      state.drives = d.drives || [];
    } catch (_) {}
    paintStatus();
  }

  let uiModeBusy = false;

  async function setUiMode(mode) {
    if (uiModeBusy) return;
    const next = mode === "stardust" ? "stardust" : "windows";
    uiModeBusy = true;
    const prev = state.uiMode === "stardust" ? "stardust" : "windows";
    state.uiMode = next;
    refreshViewMenu();
    toast("Switching…");
    try {
      const body =
        next === "stardust"
          ? { mode: "stardust", replace: true, hideDesktopIcons: true }
          : { mode: "windows", replace: false, hideDesktopIcons: false };
      const data = await apiJSON("POST", "/api/ui-mode", body, 12000);
      state.uiMode = (data && data.mode) || next;
      state.meta.replaceArmed = !!(data && data.replaceArmed);
      state.meta.hideDesktopIcons = !!(data && data.hideDesktopIcons);
      try {
        const meta = await api.get("/api/meta");
        Object.assign(state.meta, meta || {});
        state.uiMode = state.meta.uiMode === "stardust" ? "stardust" : "windows";
      } catch (_) {}
      if (data && data.warning) toast("StarDust UI without Explorer hook: " + data.warning);
      else if (next === "windows") toast("Windows Explorer restored — desktop icons and folder opens.");
      else toast("StarDust UI on — desktop icons hidden. Failsafe Ctrl+Shift+F12.");
      refreshViewMenu();
      paintStatus();
      const v = focusedView();
      if (next === "stardust" && v && !v.cns && state.meta.desktopPath) {
        navigateView(v, state.meta.desktopPath, true);
      }
      applyShellClass();
    } catch (e) {
      state.uiMode = prev;
      refreshViewMenu();
      paintStatus();
      applyShellClass();
      const msg = errText(e);
      toast(/timed out/i.test(msg) ? "Switch timed out — try again." : msg);
    } finally {
      uiModeBusy = false;
    }
  }

  async function boot() {
    applyShellClass();
    bindMenus();
    bindSearch();
    bindKeys();
    window.addEventListener("resize", resizeViews);
    document.addEventListener("visibilitychange", () => {
      if (document.visibilityState === "visible") resumeFromHost();
    });
    window.addEventListener("pageshow", resumeFromHost);
    window.addEventListener("focus", resumeFromHost);
    document.addEventListener("dragover", (e) => e.preventDefault());

    try {
      const meta = await api.get("/api/meta");
      Object.assign(state.meta, meta || {});
      state.uiMode = state.meta.uiMode === "stardust" ? "stardust" : "windows";
      applyShellClass();
    } catch (_) {
      banner("Host API not ready — chrome is live; stars appear when /api/meta and /api/children respond.");
    }

    let saved = {};
    try {
      saved = unwrapLayout(await api.get("/api/layout"));
    } catch (_) {}

    state.mode = saved.mode === "3d" ? "3d" : "2d";
    state.agentOpen = saved.agentOpen !== false;
    state.agentWidth = saved.agentWidth || 380;
    state.agentModel = saved.agentModel || saved.model || "composer-2.5";
    state.sortMode = saved.sortMode || "galaxy";
    state.starPos = normalizeStarPos(
      saved.starPos || {
        "2d": saved.positions2d || {},
        "3d": saved.positions3d || {},
      }
    );
    state.starLooks = saved.starLooks || {};
    state.pins = saved.pins || [];
    const seenPin = new Set();
    state.pins = state.pins.filter((p) => {
      const k = p.permanent
        ? "p:" + p.kind + ":" + pathKey(p.path)
        : "t:" + (p.viewId || p.winId || p.path);
      if (seenPin.has(k)) return false;
      seenPin.add(k);
      return true;
    });
    state.threads = saved.threads || [];
    refreshModeButton();
    refreshViewMenu();

    const app = {
      state,
      api,
      toast,
      errText,
      scheduleSave,
      focusedView,
      viewById,
      focusWindow,
      tempPinWindow,
      togglePermPin,
      permPinFor,
      paintPinStars,
      resizeViews,
      resumeFromHost,
      editor: null,
      agent: null,
      confirmDirty,
      confirmChoice,
    };
    window.SD.app = app;
    SD.editor = SDEditor.init(app);
    app.editor = SD.editor;
    SD.agent = SDAgent.init(app);
    app.agent = SD.agent;

    const q = new URLSearchParams(location.search || "");
    const openQ = q.get("open");
    const deskQ = q.get("desktop") === "1";
    state.ephemeral = !!openQ && !deskQ;
    applyShellClass();
    const root =
      state.uiMode === "stardust" ? sessionRoot() || state.meta.defaultRoot || "" : state.meta.defaultRoot || "";
    if (openQ && !deskQ) {
      const v = makeView(openQ, openQ);
      state.split = { type: "leaf", viewId: v.id };
    } else if (saved.views && saved.views.length) {
      saved.views.forEach((sv) => {
        const v = makeView(sv.root || root, sv.path || sv.root || root, sv.camera);
        v.id = sv.id || v.id;
        v.minimized = !!sv.minimized;
        v.mode = sv.mode === "3d" || (!sv.mode && saved.mode === "3d") ? "3d" : "2d";
      });
      state.split = saved.split || { type: "leaf", viewId: state.views[0].id };
      state.views.forEach((v) => {
        const n = parseInt(String(v.id).replace(/\D/g, ""), 10);
        if (Number.isFinite(n) && n >= state.vid) state.vid = n + 1;
      });
    } else {
      const p = deskQ ? state.meta.desktopPath || sessionRoot() || root : root;
      const v = makeView(p, p);
      state.split = { type: "leaf", viewId: v.id };
    }
    state.views.forEach((v) => {
      if (!v.minimized) return;
      const pinned = state.pins.some(
        (p) => p.kind === "view" && p.minimized && (p.viewId === v.id || p.path === v.path)
      );
      if (!pinned) v.minimized = false;
    });
    function pruneMin(node) {
      if (!node) return null;
      if (node.type === "leaf") {
        const v = viewById(node.viewId);
        return v && !v.minimized ? node : null;
      }
      const a = pruneMin(node.a);
      const b = pruneMin(node.b);
      if (!a) return b;
      if (!b) return a;
      return { type: "split", axis: splitAxisOf(node), ratio: node.ratio, a, b };
    }
    state.split = pruneMin(state.split);
    if (!state.split) {
      const live = latestUnminimized() || makeView(root, root);
      state.split = { type: "leaf", viewId: live.id };
    }
    remount();
    refreshViewMenu();
    paintStatus();
    setInterval(() => {
      api.post("/api/ui-pulse").catch(() => {});
    }, 1000);
    setInterval(refreshHealth, 2000);
    setInterval(pollUiInbox, 200);
    if (saved.floats && saved.floats.length) {
      SD.editor.restore(saved.floats).catch(() => {});
    }
    paintPins();
  }

  function uiStars(view) {
    const c = cosmosOf(view);
    if (!c || typeof c.stars !== "function") return [];
    return c.stars() || [];
  }

  function uiNormalizePath(path) {
    const raw = String(path || "").trim();
    if (!raw) return "";
    if (/^this pc$/i.test(raw) || raw === THIS_PC) return THIS_PC;
    return raw;
  }

  function uiResolveStars(view, names, paths) {
    const stars = uiStars(view);
    const out = [];
    const seen = new Set();
    function add(star) {
      if (!star) return;
      const key = star.path || star.name;
      if (!key || seen.has(key)) return;
      seen.add(key);
      out.push(star);
    }
    (names || []).forEach((n) => {
      const want = String(n || "");
      if (!want) return;
      add(stars.find((s) => s.name === want) || stars.find((s) => s.path === want));
    });
    (paths || []).forEach((p) => {
      const want = uiNormalizePath(p);
      if (!want) return;
      add(stars.find((s) => s.path === want) || stars.find((s) => s.name === want));
    });
    return out;
  }

  function lookAtView() {
    const view = focusedView();
    if (!view) return { ok: false, error: "no focused cosmos" };
    const c = cosmosOf(view);
    const stars = uiStars(view);
    const names = stars.map((s) => s.name).filter(Boolean);
    const selected =
      c && typeof c.getSelection === "function"
        ? (c.getSelection() || []).map((s) => s && s.name).filter(Boolean)
        : [];
    let image = "";
    if (c && typeof c.captureDataUrl === "function") {
      image = c.captureDataUrl({ type: "image/jpeg", quality: 0.72, maxEdge: 1280 }) || "";
    } else {
      const el = (c && (c.canvas || c.domElement)) || null;
      if (el && typeof el.toDataURL === "function") {
        try {
          image = el.toDataURL("image/jpeg", 0.72);
        } catch (_) {}
      }
    }
    return {
      ok: true,
      path: view.path === THIS_PC ? "This PC" : view.path,
      mode: viewMode(view),
      names,
      selected,
      image,
      mimeType: "image/jpeg",
    };
  }

  async function uiNavigate(path) {
    const view = focusedView();
    if (!view) return { ok: false, error: "no focused cosmos" };
    const dest = uiNormalizePath(path);
    if (!dest) return { ok: false, error: "path is required" };
    await navigateView(view, dest, true);
    return { ok: true, path: view.path === THIS_PC ? "This PC" : view.path };
  }

  async function uiOpen(names, paths) {
    const view = focusedView();
    if (!view) return { ok: false, error: "no focused cosmos" };
    const stars = uiResolveStars(view, names, paths);
    if (!stars.length) return { ok: false, error: "no matching stars in the focused cosmos" };
    await openStars(view, stars);
    return {
      ok: true,
      names: stars.map((s) => s.name),
      paths: stars.map((s) => s.path),
    };
  }

  function uiSelect(names) {
    const view = focusedView();
    if (!view) return { ok: false, error: "no focused cosmos" };
    const c = cosmosOf(view);
    if (!c || typeof c.selectNames !== "function") {
      return { ok: false, error: "focused view has no cosmos" };
    }
    const list = Array.isArray(names) ? names.map(String) : [];
    c.selectNames(list);
    return { ok: true, names: list };
  }

  function uiPin(names, paths) {
    const view = focusedView();
    if (!view) return { ok: false, error: "no focused cosmos" };
    const stars = uiResolveStars(view, names, paths);
    if (!stars.length) return { ok: false, error: "no matching stars in the focused cosmos" };
    pinStars(view, stars);
    return {
      ok: true,
      names: stars.map((s) => s.name),
      paths: stars.map((s) => s.path),
    };
  }

  async function reloadViewsAt(dir) {
    const want = pathKey(dir);
    if (!want) return;
    const views = (state.views || []).filter((v) => v && pathKey(v.path) === want);
    for (const view of views) await loadChildren(view);
  }

  async function uiCopy(args) {
    args = args || {};
    const dest = String(args.dest || args.to || "").trim();
    const sources = [];
    const seen = new Set();
    function addPath(p) {
      const n = uiNormalizePath(p);
      if (!n || n === THIS_PC) return;
      const k = pathKey(n);
      if (seen.has(k)) return;
      seen.add(k);
      sources.push(n);
    }
    if (typeof args.path === "string") addPath(args.path);
    (Array.isArray(args.paths) ? args.paths : []).forEach(addPath);
    const view = focusedView();
    if (args.names && view) {
      uiResolveStars(view, args.names, []).forEach((s) => addPath(s.path));
    }
    if (!sources.length) return { ok: false, error: "source path is required" };
    const body = dest ? { paths: sources, dest } : { paths: sources };
    const data = await api.post("/api/duplicate", body);
    const copied = (data && data.copied) || [];
    const reload = new Set();
    copied.forEach((row) => {
      const folder = parentPath(row && row.to);
      if (folder) reload.add(folder);
    });
    if (dest) reload.add(dest);
    if (!dest) sources.forEach((p) => {
      const folder = parentPath(p);
      if (folder) reload.add(folder);
    });
    if (view && view.path && view.path !== THIS_PC) reload.add(view.path);
    for (const folder of reload) await reloadViewsAt(folder);
    return {
      ok: true,
      names: copied.map((r) => r && r.name).filter(Boolean),
      paths: copied.map((r) => r && r.to).filter(Boolean),
      dest: dest || (copied[0] && parentPath(copied[0].to)) || "",
    };
  }

  async function runUiCommand(cmd) {
    const id = cmd && cmd.id;
    const op = cmd && cmd.op;
    const args = (cmd && cmd.args) || {};
    const base = { id, op };
    try {
      let out;
      if (op === "look-at-view") out = lookAtView();
      else if (op === "navigate") out = await uiNavigate(args.path);
      else if (op === "open") out = await uiOpen(args.names, args.paths);
      else if (op === "select") out = uiSelect(args.names);
      else if (op === "pin") out = uiPin(args.names, args.paths);
      else if (op === "copy") out = await uiCopy(args);
      else out = { ok: false, error: "unknown op" };
      return Object.assign(base, out);
    } catch (e) {
      return Object.assign(base, { ok: false, error: errText(e) });
    }
  }

  let uiInboxBusy = false;
  async function pollUiInbox() {
    if (uiInboxBusy) return;
    uiInboxBusy = true;
    try {
      const data = await api.get("/api/ui/inbox");
      const cmds = (data && data.commands) || [];
      for (const cmd of cmds) {
        const result = await runUiCommand(cmd);
        try {
          await api.post("/api/ui/result", result);
        } catch (_) {}
      }
    } catch (_) {
    } finally {
      uiInboxBusy = false;
    }
  }

  window.SD = {
    boot,
    state,
    api,
    toast,
    errText,
    navigate: (path) => uiNavigate(path),
    open: (names, paths) => uiOpen(names, paths),
    select: (names) => uiSelect(names),
    pin: (names, paths) => uiPin(names, paths),
    copy: (args) => uiCopy(args),
    lookAtView,
  };
})();
