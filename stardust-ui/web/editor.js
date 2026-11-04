(function () {
  const MONACO_VER = "0.52.2";
  const VS = "https://cdn.jsdelivr.net/npm/monaco-editor@" + MONACO_VER + "/min/vs";

  function extOf(p) {
    const n = (p || "").split(/[/\\]/).pop() || "";
    const i = n.lastIndexOf(".");
    return i >= 0 ? n.slice(i).toLowerCase() : "";
  }

  function langOf(p) {
    const map = {
      ".rs": "rust",
      ".js": "javascript",
      ".ts": "typescript",
      ".tsx": "typescript",
      ".json": "json",
      ".css": "css",
      ".html": "html",
      ".htm": "html",
      ".md": "markdown",
      ".py": "python",
      ".toml": "ini",
      ".yml": "yaml",
      ".yaml": "yaml",
      ".c": "c",
      ".h": "c",
      ".cpp": "cpp",
      ".xml": "xml",
      ".svg": "xml",
      ".sh": "shell",
      ".ps1": "powershell",
      ".sql": "sql",
      ".go": "go",
      ".java": "java",
    };
    return map[extOf(p)] || "plaintext";
  }

  let monacoP = null;
  function loadMonaco() {
    if (window.monaco) return Promise.resolve(window.monaco);
    if (monacoP) return monacoP;
    monacoP = new Promise((resolve, reject) => {
      window.MonacoEnvironment = {
        getWorkerUrl() {
          const body =
            "self.MonacoEnvironment={baseUrl:'https://cdn.jsdelivr.net/npm/monaco-editor@" +
            MONACO_VER +
            "/min/'};importScripts('" +
            VS +
            "/base/worker/workerMain.js');";
          return URL.createObjectURL(new Blob([body], { type: "text/javascript" }));
        },
      };
      const s = document.createElement("script");
      s.src = VS + "/loader.js";
      s.onload = () => {
        window.require.config({ paths: { vs: VS } });
        window.require(["vs/editor/editor.main"], () => resolve(window.monaco));
      };
      s.onerror = () => reject(new Error("Monaco failed to load"));
      document.head.appendChild(s);
    });
    return monacoP;
  }

  window.SDEditor = {
    loadMonaco,
    init(app) {
      const floats = document.getElementById("floats");
      const wins = [];
      let z = 30;
      let idc = 1;
      let focused = null;

      function bump(w) {
        if (!w || !w.el) return;
        w.el.style.zIndex = String(++z);
        wins.forEach((x) => x.el && x.el.classList.toggle("focused", x === w));
        focused = w;
        app.focusWindow(w);
      }

      function layoutDock(view) {
        if (!view || !view.rootEl) return;
        const docked = wins.filter((w) => w.docked && w.viewId === view.id);
        const dock = view.rootEl.querySelector(".vdock");
        if (!dock) return;
        const tabs = dock.querySelector(".dtabs");
        const body = dock.querySelector(".dbody");
        dock.hidden = docked.length === 0;
        tabs.innerHTML = "";
        if (!docked.length) return;
        if (!docked.some((w) => w.tabOn)) docked[0].tabOn = true;
        docked.forEach((w) => {
          const b = document.createElement("button");
          b.className = "dtab" + (w.tabOn ? " on" : "");
          b.type = "button";
          b.textContent = (w.dirty ? "● " : "") + w.title;
          b.onclick = () => {
            docked.forEach((x) => (x.tabOn = x === w));
            bump(w);
            layoutDock(view);
          };
          tabs.appendChild(b);
        });
        const on = docked.find((w) => w.tabOn) || docked[0];
        const saveBtn = document.createElement("button");
        saveBtn.type = "button";
        saveBtn.className = "dtab";
        saveBtn.textContent = "Save";
        saveBtn.title = "Save (Ctrl+S)";
        saveBtn.disabled = !on.dirty || !!on.readOnly;
        saveBtn.onclick = () => save(on);
        tabs.appendChild(saveBtn);
        const undockBtn = document.createElement("button");
        undockBtn.type = "button";
        undockBtn.className = "dtab";
        undockBtn.textContent = "Undock";
        undockBtn.onclick = () => undock(on);
        tabs.appendChild(undockBtn);
        body.innerHTML = "";
        on.host.style.display = "block";
        on.host.style.width = "100%";
        on.host.style.height = "100%";
        body.appendChild(on.host);
        body.onpointerdown = () => bump(on);
        if (on.editor && on.editor.layout) on.editor.layout();
      }

      function undock(w) {
        w.docked = false;
        w.el.hidden = false;
        const bodyEl = w.el.querySelector(".float-body");
        if (bodyEl) bodyEl.appendChild(w.host);
        else w.el.appendChild(w.host);
        const view = app.viewById(w.viewId);
        if (view) layoutDock(view);
        if (w.editor && w.editor.layout) w.editor.layout();
        bump(w);
        app.scheduleSave();
      }

      function dock(w) {
        const view = app.focusedView();
        if (!view) return;
        w.docked = true;
        w.viewId = view.id;
        w.el.hidden = true;
        w.tabOn = true;
        wins.filter((x) => x.viewId === view.id && x !== w).forEach((x) => (x.tabOn = false));
        layoutDock(view);
        app.scheduleSave();
      }

      function startResize(w, dir, e, handle) {
        e.preventDefault();
        e.stopPropagation();
        bump(w);
        if (w.max) {
          w.max = false;
          w._b = null;
        }
        try {
          handle.setPointerCapture(e.pointerId);
        } catch (_) {}
        const sx = e.clientX;
        const sy = e.clientY;
        const ox = w.x;
        const oy = w.y;
        const ow = w.w;
        const oh = w.h;
        const minW = 280;
        const minH = 160;
        function mv(ev) {
          const dx = ev.clientX - sx;
          const dy = ev.clientY - sy;
          let x = ox;
          let y = oy;
          let ww = ow;
          let hh = oh;
          if (dir.indexOf("e") >= 0) ww = Math.max(minW, ow + dx);
          if (dir.indexOf("s") >= 0) hh = Math.max(minH, oh + dy);
          if (dir.indexOf("w") >= 0) {
            ww = Math.max(minW, ow - dx);
            x = ox + (ow - ww);
          }
          if (dir.indexOf("n") >= 0) {
            hh = Math.max(minH, oh - dy);
            y = oy + (oh - hh);
          }
          w.x = x;
          w.y = y;
          w.w = ww;
          w.h = hh;
          place(w);
        }
        function up() {
          handle.removeEventListener("pointermove", mv);
          handle.removeEventListener("pointerup", up);
          app.scheduleSave();
        }
        handle.addEventListener("pointermove", mv);
        handle.addEventListener("pointerup", up);
      }

      function addResizeHandles(el, w) {
        ["n", "s", "e", "w", "ne", "nw", "se", "sw"].forEach((dir) => {
          const h = document.createElement("div");
          h.className = "float-resize float-resize-" + dir;
          h.addEventListener("pointerdown", (e) => startResize(w, dir, e, h));
          el.appendChild(h);
        });
      }

      function chrome(w) {
        const el = document.createElement("div");
        el.className = "float-win";
        el.style.left = w.x + "px";
        el.style.top = w.y + "px";
        el.style.width = w.w + "px";
        el.style.height = w.h + "px";
        const title = document.createElement("div");
        title.className = "float-title";
        const t = document.createElement("span");
        t.className = "t";
        t.textContent = w.title;
        const dirty = document.createElement("span");
        dirty.className = "dirty";
        const saveBtn = document.createElement("button");
        saveBtn.type = "button";
        saveBtn.title = "Save (Ctrl+S)";
        saveBtn.textContent = "Save";
        saveBtn.disabled = true;
        saveBtn.onclick = (e) => {
          e.stopPropagation();
          save(w);
        };
        const dockBtn = document.createElement("button");
        dockBtn.type = "button";
        dockBtn.title = "Dock";
        dockBtn.textContent = "▣";
        dockBtn.onclick = (e) => {
          e.stopPropagation();
          dock(w);
        };
        const pinBtn = document.createElement("button");
        pinBtn.type = "button";
        pinBtn.dataset.act = "pin-file";
        pinBtn.dataset.path = w.path || "";
        pinBtn.textContent = "☆";
        pinBtn.title = "Pin";
        pinBtn.onclick = (e) => {
          e.stopPropagation();
          if (app.togglePermPin) {
            app.togglePermPin({ kind: "file", path: w.path, title: w.title });
          }
        };
        const minBtn = document.createElement("button");
        minBtn.type = "button";
        minBtn.title = "Minimize";
        minBtn.textContent = "–";
        minBtn.onclick = (e) => {
          e.stopPropagation();
          app.tempPinWindow(w);
        };
        const maxBtn = document.createElement("button");
        maxBtn.type = "button";
        maxBtn.textContent = "□";
        maxBtn.onclick = (e) => {
          e.stopPropagation();
          w.max = !w.max;
          if (w.max) {
            w._b = { x: w.x, y: w.y, w: w.w, h: w.h };
            w.x = 8;
            w.y = 8;
            const main = document.getElementById("main").getBoundingClientRect();
            w.w = Math.max(280, main.width - 16);
            w.h = Math.max(140, main.height - 16);
          } else if (w._b) {
            Object.assign(w, w._b);
          }
          place(w);
        };
        const closeBtn = document.createElement("button");
        closeBtn.type = "button";
        closeBtn.textContent = "×";
        closeBtn.onclick = (e) => {
          e.stopPropagation();
          close(w.id);
        };
        title.append(t, dirty, saveBtn, dockBtn, pinBtn, minBtn, maxBtn, closeBtn);
        if (app.paintPinStars) app.paintPinStars();
        const body = document.createElement("div");
        body.className = "float-body";
        const host = document.createElement("div");
        host.style.height = "100%";
        body.appendChild(host);
        el.append(title, body);
        addResizeHandles(el, w);
        floats.appendChild(el);
        w.el = el;
        w.host = host;
        w.titleEl = t;
        w.dirtyEl = dirty;
        w.saveBtn = saveBtn;

        title.addEventListener("pointerdown", (e) => {
          if (e.target.closest("button")) return;
          bump(w);
          try {
            title.setPointerCapture(e.pointerId);
          } catch (_) {}
          const sx = e.clientX;
          const sy = e.clientY;
          const ox = w.x;
          const oy = w.y;
          function mv(ev) {
            w.x = ox + ev.clientX - sx;
            w.y = oy + ev.clientY - sy;
            place(w);
          }
          function up() {
            title.removeEventListener("pointermove", mv);
            title.removeEventListener("pointerup", up);
            app.scheduleSave();
          }
          title.addEventListener("pointermove", mv);
          title.addEventListener("pointerup", up);
        });
        el.addEventListener("pointerdown", () => bump(w));
      }

      function place(w) {
        if (!w.el) return;
        w.el.style.left = w.x + "px";
        w.el.style.top = w.y + "px";
        w.el.style.width = w.w + "px";
        w.el.style.height = w.h + "px";
        if (w.editor && w.editor.layout) w.editor.layout();
      }

      function markDirty(w, d) {
        w.dirty = d;
        if (w.dirtyEl) w.dirtyEl.textContent = d ? "●" : "";
        if (w.saveBtn) w.saveBtn.disabled = !d || !!w.readOnly;
        if (w.docked) {
          const view = app.viewById(w.viewId);
          if (view) layoutDock(view);
        }
      }

      function mountTextarea(w, content) {
        const ta = document.createElement("textarea");
        ta.className = "fallback-editor";
        ta.value = content;
        ta.addEventListener("input", () => markDirty(w, ta.value !== w.base));
        w.host.appendChild(ta);
        w.editor = {
          getValue: () => ta.value,
          layout() {},
          dispose() {
            ta.remove();
          },
          hasTextFocus() {
            return document.activeElement === ta;
          },
        };
        w.base = content;
      }

      async function mountEditor(w, content) {
        try {
          const monaco = await loadMonaco();
          monaco.editor.defineTheme("stardust-dark", {
            base: "vs-dark",
            inherit: true,
            rules: [],
            colors: {
              "editor.background": "#0b1018",
              "editor.foreground": "#e8eefc",
              focusBorder: "#7eb8ff",
            },
          });
          monaco.editor.setTheme("stardust-dark");
          w.editor = monaco.editor.create(w.host, {
            value: content,
            language: langOf(w.path),
            automaticLayout: true,
            minimap: { enabled: false },
            fontSize: 13,
            theme: "stardust-dark",
            readOnly: !!w.readOnly,
          });
          w.base = content;
          if (!w.readOnly) {
            w.editor.onDidChangeModelContent(() => markDirty(w, w.editor.getValue() !== w.base));
          }
        } catch (_) {
          mountTextarea(w, content);
          if (!w.readOnly) app.toast("Monaco unavailable — using a plain text editor.");
        }
      }

      async function save(w) {
        if (!w || w.kind !== "editor" || !w.editor || w.readOnly) return;
        const content = w.editor.getValue();
        await app.api.put("/api/file", { path: w.path, content });
        w.base = content;
        markDirty(w, false);
      }

      function close(id, force) {
        const w = wins.find((x) => x.id === id);
        if (!w) return Promise.resolve(true);
        const go = () => {
          if (w.editor && w.editor.dispose) w.editor.dispose();
          w.el.remove();
          const i = wins.indexOf(w);
          if (i >= 0) wins.splice(i, 1);
          if (focused === w) focused = wins[wins.length - 1] || null;
          const view = app.viewById(w.viewId);
          if (view) layoutDock(view);
          app.scheduleSave();
          return true;
        };
        if (!force && w.dirty) {
          return app.confirmDirty(w.title).then(async (choice) => {
            if (choice === "cancel") return false;
            if (choice === "save") await save(w);
            return go();
          });
        }
        go();
        return Promise.resolve(true);
      }

      async function open(path, opts) {
        opts = opts || {};
        const existing = wins.find((x) => x.kind === "editor" && x.path === path && !x.minimized);
        if (existing) {
          if (existing.docked) {
            existing.tabOn = true;
            const view = app.viewById(existing.viewId);
            if (view) layoutDock(view);
          } else {
            existing.el.hidden = false;
            bump(existing);
          }
          return existing;
        }
        if (opts.isText === false) {
          await app.api.post("/api/open-external", { path }).catch(() => {});
          return null;
        }
        let content = "";
        let binary = opts.isText === false;
        if (!binary) {
          try {
            const data = await app.api.get("/api/file?path=" + encodeURIComponent(path));
            if (data && data.binary) binary = true;
            else content = (data && (data.content || data.text || data.raw)) || "";
          } catch (e) {
            if (e && e.body && e.body.binary) binary = true;
            else if (e && e.status === 415) binary = true;
            else {
              app.toast("Could not open file: " + app.errText(e));
              return null;
            }
          }
        }
        if (binary) {
          await app.api.post("/api/open-external", { path }).catch(() => {});
          return null;
        }
        const n = wins.filter((x) => x.kind === "editor").length;
        const w = {
          id: "w" + idc++,
          kind: "editor",
          path,
          title: path.split(/[/\\]/).pop() || path,
          x: 36 + (n % 8) * 24,
          y: 28 + (n % 8) * 24,
          w: 560,
          h: 380,
          docked: !!opts.docked,
          viewId: (app.focusedView() || {}).id,
          dirty: false,
          minimized: false,
          readOnly: !!opts.readOnly,
        };
        if (opts.bounds) Object.assign(w, opts.bounds);
        chrome(w);
        wins.push(w);
        bump(w);
        await mountEditor(w, content);
        if (w.docked) dock(w);
        app.scheduleSave();
        return w;
      }

      function arrangeableWindows() {
        return wins.filter((w) => w.kind === "editor" && !w.minimized && w.el);
      }

      function tile() {
        const list = arrangeableWindows();
        if (!list.length) {
          app.toast("Tile arranges floating file windows. Open a text file first (then Undock if it is a tab).");
          return;
        }
        list.forEach((w) => {
          if (w.docked) undock(w);
        });
        const main = document.getElementById("main").getBoundingClientRect();
        const n = list.length;
        const cols = Math.ceil(Math.sqrt(n));
        const rows = Math.ceil(n / cols);
        const ww = main.width / cols;
        const hh = main.height / rows;
        list.forEach((w, i) => {
          const c = i % cols;
          const r = Math.floor(i / cols);
          w.x = c * ww + 4;
          w.y = r * hh + 4;
          w.w = Math.max(240, ww - 8);
          w.h = Math.max(160, hh - 8);
          w.max = false;
          place(w);
        });
        app.scheduleSave();
      }

      function cascade() {
        const list = arrangeableWindows();
        if (!list.length) {
          app.toast("Cascade arranges floating file windows. Open a text file first (then Undock if it is a tab).");
          return;
        }
        list.forEach((w) => {
          if (w.docked) undock(w);
        });
        list.forEach((w, i) => {
          w.x = 24 + i * 28;
          w.y = 20 + i * 28;
          w.w = 540;
          w.h = 360;
          w.max = false;
          place(w);
        });
        app.scheduleSave();
      }

      return {
        open,
        close,
        save,
        saveFocused() {
          if (focused && focused.kind === "editor") return save(focused);
          const view = app.focusedView();
          const on = view && wins.find((w) => w.docked && w.tabOn && w.viewId === view.id);
          if (on) return save(on);
        },
        focused() {
          return focused;
        },
        blur() {
          focused = null;
        },
        isEditing() {
          const a = document.activeElement;
          if (!a) return false;
          if (a.closest(".monaco-editor") || a.tagName === "TEXTAREA" || a.tagName === "INPUT") return true;
          return !!(focused && focused.editor && focused.editor.hasTextFocus && focused.editor.hasTextFocus());
        },
        buffer(path) {
          const w = wins.find((x) => x.path === path && x.editor);
          if (!w) return null;
          return { content: w.editor.getValue(), dirty: w.dirty };
        },
        windows() {
          return wins;
        },
        arrangeCount() {
          return arrangeableWindows().length;
        },
        tile,
        cascade,
        dock,
        undock,
        hide(w) {
          w.minimized = true;
          w.el.hidden = true;
        },
        show(w) {
          w.minimized = false;
          if (!w.docked) w.el.hidden = false;
          bump(w);
        },
        serialize() {
          return wins
            .filter((w) => w.kind === "editor")
            .map((w) => ({
              path: w.path,
              bounds: { x: w.x, y: w.y, w: w.w, h: w.h },
              docked: !!w.docked,
              viewId: w.viewId,
              minimized: !!w.minimized,
            }));
        },
        async restore(list) {
          for (const f of list || []) {
            await open(f.path, { bounds: f.bounds, docked: f.docked });
            const w = wins.find((x) => x.path === f.path);
            if (w && f.minimized) {
              w.minimized = true;
              w.el.hidden = true;
            }
          }
        },
        layoutDock,
      };
    },
  };
})();
