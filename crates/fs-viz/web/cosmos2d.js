(function () {
  const GOLDEN = 2.399963229728653;

  function paneDebt(k, w, h) {
    const short = Math.max(96, Math.min(w, h));
    return k * (720 / short);
  }
  function leaveDebtNeed(w, h) { return paneDebt(140, w, h); }  // was 180
  function enterDebtNeed(w, h) { return paneDebt(100, w, h); }  // new

  function layoutGalaxy(items) {
    const dirs = items.filter((s) => s.kind === "dir");
    const files = items.filter((s) => s.kind !== "dir");
    const pos = {};
    function ring(list, r0, spread, ox) {
      list.forEach((s, i) => {
        const a = i * GOLDEN;
        const r = r0 + spread * Math.sqrt(i + 1);
        pos[s.name] = { x: ox + Math.cos(a) * r, y: Math.sin(a) * r };
      });
    }
    ring(dirs, 72, 34, -36);
    ring(files, 158, 38, 48);
    const bodies = items.map((s) => ({
      name: s.name,
      x: pos[s.name].x,
      y: pos[s.name].y,
    }));
    for (let k = 0; k < 48; k++) {
      for (let i = 0; i < bodies.length; i++) {
        let fx = 0;
        let fy = 0;
        for (let j = 0; j < bodies.length; j++) {
          if (i === j) continue;
          const dx = bodies[i].x - bodies[j].x;
          const dy = bodies[i].y - bodies[j].y;
          const d2 = dx * dx + dy * dy + 36;
          const f = 240 / d2;
          fx += dx * f;
          fy += dy * f;
        }
        bodies[i].x += fx * 0.035;
        bodies[i].y += fy * 0.035;
      }
    }
    bodies.forEach((b) => {
      pos[b.name] = { x: b.x, y: b.y };
    });
    return pos;
  }

  function starSep(kind) {
    return kind === "dir" ? 18 : 8;
  }

  function placeUnoccupied(missing, saved) {
    const occ = [];
    Object.keys(saved || {}).forEach((k) => {
      const p = saved[k];
      if (!p) return;
      occ.push({ x: p.x, y: p.y, sep: starSep(p.kind) });
    });
    let rMax = 0;
    for (let i = 0; i < occ.length; i++) {
      const d = Math.hypot(occ[i].x, occ[i].y);
      if (d > rMax) rMax = d;
    }
    const r0 = rMax + 48;
    const bodies = (missing || []).map((s, i) => {
      const a = i * GOLDEN;
      const r = r0 + 26 * Math.sqrt(i + 1);
      return {
        name: s.name,
        sep: starSep(s.kind),
        x: Math.cos(a) * r,
        y: Math.sin(a) * r,
      };
    });
    for (let k = 0; k < 24; k++) {
      for (let i = 0; i < bodies.length; i++) {
        let fx = 0;
        let fy = 0;
        const acc = (ox, oy, osep) => {
          const dx = bodies[i].x - ox;
          const dy = bodies[i].y - oy;
          const need = bodies[i].sep + osep;
          const d = Math.hypot(dx, dy);
          if (d < 0.0001) {
            const a = (i + 1) * GOLDEN;
            fx += Math.cos(a) * need;
            fy += Math.sin(a) * need;
            return;
          }
          if (d < need) {
            const m = (need - d) / d;
            fx += dx * m;
            fy += dy * m;
          }
        };
        for (let j = 0; j < occ.length; j++) acc(occ[j].x, occ[j].y, occ[j].sep);
        for (let j = 0; j < bodies.length; j++) {
          if (i !== j) acc(bodies[j].x, bodies[j].y, bodies[j].sep);
        }
        bodies[i].x += fx * 0.4;
        bodies[i].y += fy * 0.4;
      }
    }
    const pos = {};
    bodies.forEach((b) => {
      pos[b.name] = { x: b.x, y: b.y };
    });
    return pos;
  }

  function hintBlockers(stars, pending, skipName) {
    const list = [];
    (stars || []).forEach((s) => {
      if (s && s.name !== skipName) list.push({ x: s.x, y: s.y, kind: s.kind });
    });
    Object.keys(pending || {}).forEach((n) => {
      if (n === skipName) return;
      const p = pending[n];
      if (p) list.push({ x: p.x, y: p.y, kind: p.kind || "file" });
    });
    return list;
  }

  function spiralAway(x, y, blockers, kind) {
    const selfSep = starSep(kind || "dir");
    const blocked = (px, py) => {
      for (let i = 0; i < blockers.length; i++) {
        const o = blockers[i];
        const need = selfSep + starSep(o.kind);
        if (Math.hypot(px - o.x, py - o.y) < need) return true;
      }
      return false;
    };
    if (!blocked(x, y)) return { x, y };
    for (let i = 1; i < 96; i++) {
      const a = i * GOLDEN;
      const r = selfSep * 2 + 12 * Math.sqrt(i);
      const px = x + Math.cos(a) * r;
      const py = y + Math.sin(a) * r;
      if (!blocked(px, py)) return { x: px, y: py };
    }
    return { x: x + selfSep * 2, y };
  }

  function cloneMap(m) {
    const o = {};
    Object.keys(m || {}).forEach((k) => {
      const p = m[k];
      if (p) o[k] = { x: p.x, y: p.y };
    });
    return o;
  }

  function captureCanvasDataUrl(src, type, quality, maxEdge) {
    if (!src || !src.width || !src.height) return "";
    const w = src.width;
    const h = src.height;
    const edge = Math.max(w, h);
    let dw = w;
    let dh = h;
    if (maxEdge && edge > maxEdge) {
      const s = maxEdge / edge;
      dw = Math.max(1, Math.round(w * s));
      dh = Math.max(1, Math.round(h * s));
    }
    try {
      if (dw === w && dh === h) return src.toDataURL(type, quality);
      const tmp = document.createElement("canvas");
      tmp.width = dw;
      tmp.height = dh;
      const g = tmp.getContext("2d");
      if (!g) return src.toDataURL(type, quality);
      g.drawImage(src, 0, 0, dw, dh);
      return tmp.toDataURL(type, quality);
    } catch (_) {
      return "";
    }
  }

  window.Cosmos2D = {
    create(container, h) {
      const canvas = document.createElement("canvas");
      container.appendChild(canvas);
      const ctx = canvas.getContext("2d");
      const status = document.createElement("div");
      status.className = "vload";
      status.textContent = "Loading…";
      container.appendChild(status);
      const marqueeEl = document.createElement("div");
      marqueeEl.className = "marquee-rect";
      marqueeEl.hidden = true;
      container.appendChild(marqueeEl);

      const self = {
        cam: { x: 0, y: 0, scale: 1 },
        stars: [],
        dirPath: "",
        frozen: {},
        arranged: false,
        sel: new Set(),
        anchor: null,
        hover: null,
        highlight: null,
        highlightParent: null,
        ctrl: false,
        mouse: { x: 0, y: 0 },
        pending: {},
        flying: false,
        flyDir: null,
        dead: false,
        drag: null,
        loading: true,
        error: "",
        w: 1,
        h: 1,
        leaveAcc: 0,
        enterAcc: 0,
        enterTarget: null,
        lastClick: null,
        suppressDblClick: false,
        far: [],
        farSeed: "",
        live: true,
        looks: {},
        hostLayout: false,
      };

      const glyphImgs = {};
      function lookKey(p) {
        return String(p || "")
          .replace(/\//g, "\\")
          .replace(/\\+$/, "")
          .toLowerCase();
      }
      function glyphOf(star) {
        const looks = self.looks || {};
        return looks[lookKey(star.path)] || looks[star.path] || null;
      }
      function stampGlyphs() {
        self.stars.forEach((s) => {
          s.glyph = glyphOf(s);
          if (s.glyph && s.glyph.dirStyle) s.dirStyle = s.glyph.dirStyle;
        });
      }
      function glyphImage(dataUrl) {
        if (!dataUrl) return null;
        if (glyphImgs[dataUrl]) return glyphImgs[dataUrl];
        const im = new Image();
        im.src = dataUrl;
        glyphImgs[dataUrl] = im;
        return im;
      }

      function fit() {
        const dpr = window.devicePixelRatio || 1;
        const w = Math.max(1, container.clientWidth || container.getBoundingClientRect().width);
        const hgt = Math.max(1, container.clientHeight || container.getBoundingClientRect().height);
        canvas.width = Math.floor(w * dpr);
        canvas.height = Math.floor(hgt * dpr);
        ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
        self.w = w;
        self.h = hgt;
      }

      function toScreen(p) {
        return {
          x: (p.x - self.cam.x) * self.cam.scale + self.w / 2,
          y: (p.y - self.cam.y) * self.cam.scale + self.h / 2,
        };
      }

      function toWorld(cx, cy) {
        const r = container.getBoundingClientRect();
        const x = cx - r.left;
        const y = cy - r.top;
        return {
          x: (x - self.w / 2) / self.cam.scale + self.cam.x,
          y: (y - self.h / 2) / self.cam.scale + self.cam.y,
        };
      }

      function isCnsWord(s) {
        return !!(s && (s.role === "word" || String(s.path || "").indexOf("cns:word:") === 0));
      }
      function starR(s) {
        if (isCnsWord(s)) {
          return Math.max(3.2, Math.min(7, 3.5 * Math.sqrt(Math.max(0.18, self.cam.scale))));
        }
        const base = s.kind === "dir" ? 16 : 6.5;
        return Math.max(4, Math.min(34, base * Math.sqrt(self.cam.scale)));
      }

      function hit(cx, cy) {
        const r = container.getBoundingClientRect();
        const x = cx - r.left;
        const y = cy - r.top;
        let best = null;
        let bestD = 1e9;
        for (const s of self.stars) {
          const p = toScreen(s);
          const d = Math.hypot(p.x - x, p.y - y);
          const rad = starR(s) + (s.kind === "dir" ? 14 : 8);
          if (d < rad && d < bestD) {
            best = s;
            bestD = d;
          }
        }
        return best;
      }

      function emitPos() {
        if (h.onPositions) h.onPositions(self.dirPath, cloneMap(self.frozen));
      }

      function setMarquee(x0, y0, x1, y1) {
        marqueeEl.hidden = false;
        marqueeEl.style.left = Math.min(x0, x1) + "px";
        marqueeEl.style.top = Math.min(y0, y1) + "px";
        marqueeEl.style.width = Math.abs(x1 - x0) + "px";
        marqueeEl.style.height = Math.abs(y1 - y0) + "px";
      }
      function hideMarquee() {
        marqueeEl.hidden = true;
      }

      function applyMap(map) {
        self.stars.forEach((s) => {
          const p = map[s.name];
          if (p) {
            s.x = p.x;
            s.y = p.y;
          }
        });
      }

      function setStatus() {
        if (self.loading) {
          status.hidden = false;
          status.className = self.loadClass || "vload";
          status.textContent = self.loadText || "Loading…";
        } else if (self.error) {
          status.hidden = false;
          status.className = "verror";
          status.textContent = self.error;
        } else {
          status.hidden = true;
        }
      }

      function setScene(opts) {
        self.loading = !!opts.loading;
        self.error = opts.error || "";
        if (opts.loading) {
          if (opts.loadText) self.loadText = opts.loadText;
          if (opts.loadClass) self.loadClass = opts.loadClass;
        } else {
          self.loadText = "";
          self.loadClass = "";
        }
        setStatus();
        if (self.loading) return;
        self.hostLayout = !!opts.hostLayout;
        self.dirPath = opts.dirPath || "";
        const children = (opts.children || []).map((c) => Object.assign({}, c));
        const saved = cloneMap(opts.savedPositions || {});
        const missing = children.filter((c) => !saved[c.name]);
        let map = saved;
        let changed = false;
        if (opts.hostLayout) {
          map = {};
          children.forEach((c) => {
            map[c.name] = { x: c.x || 0, y: c.y || 0 };
          });
          changed = false;
        } else if (!Object.keys(saved).length && children.length) {
          map = layoutGalaxy(children);
          changed = true;
        } else if (missing.length) {
          const occ = cloneMap(saved);
          children.forEach((c) => {
            if (occ[c.name]) occ[c.name].kind = c.kind;
          });
          const pend = self.pending || {};
          const rest = [];
          missing.forEach((c) => {
            const p = pend[c.name];
            if (p) occ[c.name] = { x: p.x, y: p.y, kind: c.kind };
            else rest.push(c);
          });
          map = Object.assign({}, saved, rest.length ? placeUnoccupied(rest, occ) : {});
          changed = true;
        }
        if (!opts.hostLayout) {
          const pend = self.pending || {};
          const names = new Set(children.map((c) => c.name));
          Object.keys(pend).forEach((name) => {
            if (!names.has(name)) return;
            map[name] = { x: pend[name].x, y: pend[name].y };
            changed = true;
          });
        }
        self.pending = {};
        self.frozen = cloneMap(map);
        self.arranged = false;
        const Look = window.SDLook;
        self.stars = children.map((c) =>
          Object.assign({}, c, {
            x: map[c.name] ? map[c.name].x : 0,
            y: map[c.name] ? map[c.name].y : 0,
            twinkle0: Look ? (Look.hashName(c.name) % 1000) / 1000 : Math.random(),
            dirStyle: c.kind === "dir" && Look ? Look.dirStyle(c.name) : "nebula",
          })
        );
        if (opts.looks) self.looks = opts.looks;
        stampGlyphs();
        if (Look && self.farSeed !== self.dirPath) {
          self.far = Look.makeFarField(self.dirPath, 560);
          self.farSeed = self.dirPath;
        }
        const keep = new Set(self.stars.map((s) => s.name));
        [...self.sel].forEach((n) => {
          if (!keep.has(n)) self.sel.delete(n);
        });
        self.anchor = null;
        self.hover = null;
        if (changed && !opts.ephemeral) emitPos();
        if (opts.resetCamera) self.cam = { x: 0, y: 0, scale: 1 };
        else if (opts.camera) self.cam = Object.assign({}, self.cam, opts.camera);
      }

      function arrangeBy(key) {
        const items = self.stars.slice();
        if (key === "name") items.sort((a, b) => a.name.localeCompare(b.name));
        else if (key === "type")
          items.sort((a, b) => a.kind.localeCompare(b.kind) || a.name.localeCompare(b.name));
        else if (key === "size") items.sort((a, b) => (b.size || 0) - (a.size || 0));
        const n = items.length;
        if (!n) return;
        if (n <= 14) {
          const r = 40 + n * 12;
          items.forEach((s, i) => {
            const a = (i / n) * Math.PI * 2 - Math.PI / 2;
            s.x = Math.cos(a) * r;
            s.y = Math.sin(a) * r;
          });
        } else {
          items.forEach((s, i) => {
            s.x = (i - (n - 1) / 2) * 88;
            s.y = (i % 2 === 0 ? -28 : 28);
          });
        }
        self.arranged = true;
      }

      function galaxy() {
        applyMap(self.frozen);
        self.arranged = false;
      }

      function spatialRange(a, b) {
        const pa = toScreen(a);
        const pb = toScreen(b);
        const minx = Math.min(pa.x, pb.x);
        const maxx = Math.max(pa.x, pb.x);
        const miny = Math.min(pa.y, pb.y);
        const maxy = Math.max(pa.y, pb.y);
        return self.stars.filter((s) => {
          const p = toScreen(s);
          return p.x >= minx && p.x <= maxx && p.y >= miny && p.y <= maxy;
        });
      }

      function announceSel() {
        if (h.onSelect)
          h.onSelect([...self.sel].map((n) => self.stars.find((s) => s.name === n)).filter(Boolean));
      }

      function draw() {
        if (self.dead) return;
        const Look = window.SDLook;
        const now = performance.now();
        const w = self.w || 1;
        const hgt = self.h || 1;
        ctx.clearRect(0, 0, w, hgt);
        ctx.globalCompositeOperation = "source-over";
        ctx.fillStyle = "rgba(5, 7, 13, 0.06)";
        ctx.fillRect(0, 0, w, hgt);
        ctx.globalCompositeOperation = "lighter";
        if (Look && self.far && self.far.length) {
          Look.drawFarField2d(ctx, self.far, toScreen, w, hgt, now, self.cam.scale);
        }

        const zoomA = Math.max(0.28, Math.min(0.92, 0.32 + (self.cam.scale - 0.55) * 0.42));
        const labelFont = '"Bahnschrift", "Segoe UI Variable Text", "Segoe UI", sans-serif';
        let cnsVis = 0;
        if (self.hostLayout) {
          for (let i = 0; i < self.stars.length; i++) {
            const s = self.stars[i];
            if (!isCnsWord(s)) continue;
            const p = toScreen(s);
            if (p.x < -50 || p.y < -50 || p.x > w + 50 || p.y > hgt + 50) continue;
            cnsVis++;
            if (cnsVis > 35) break;
          }
        }
        const cnsLabelsOpen = !self.hostLayout || cnsVis <= 35 || self.cam.scale * 5 >= 36;
        for (const s of self.stars) {
          const p = toScreen(s);
          if (p.x < -50 || p.y < -50 || p.x > w + 50 || p.y > hgt + 50) continue;
          const r = starR(s);
          const on =
            self.hover === s ||
            self.highlight === s.name ||
            self.highlightParent === s.name ||
            self.sel.has(s.name);
          const phase = s.twinkle0 || 0;
          const glyph = s.glyph;
          const word = isCnsWord(s);
          if (glyph && glyph.image) {
            const im = glyphImage(glyph.image);
            if (im && im.complete && im.naturalWidth) {
              ctx.save();
              ctx.globalCompositeOperation = "source-over";
              const side = r * (s.kind === "dir" ? 3.2 : 2.6);
              ctx.beginPath();
              ctx.arc(p.x, p.y, side * 0.52, 0, Math.PI * 2);
              ctx.clip();
              ctx.drawImage(im, p.x - side / 2, p.y - side / 2, side, side);
              ctx.restore();
            }
          } else if (Look) {
            const hsh = Look.hashName(s.name);
            if (s.kind === "dir") {
              const br = Look.twinkleAt(phase, now, false);
              Look.paintDirNebula(ctx, p.x, p.y, r, {
                on,
                breathe: br,
                style: (glyph && glyph.dirStyle) || s.dirStyle || Look.dirStyle(s.name),
                rot: phase * 0.4,
                seed: hsh,
              });
            } else {
              const tw = Look.twinkleAt(phase, now, true);
              const look = Look.starLook(s.name);
              Look.paintFileStar(ctx, p.x, p.y, r, {
                on,
                twinkle: tw,
                spec: (glyph && glyph.spec) || look.spec,
                morph: (glyph && glyph.morph) || look.morph,
              });
            }
          }
          if (self.sel.has(s.name)) {
            ctx.save();
            ctx.globalCompositeOperation = "lighter";
            const hr = r * (s.kind === "dir" ? 2.6 : 3.8);
            const grd = ctx.createRadialGradient(p.x, p.y, Math.max(1, r * 0.15), p.x, p.y, hr);
            grd.addColorStop(0, "rgba(210, 240, 255, 0.45)");
            grd.addColorStop(0.45, "rgba(120, 190, 255, 0.22)");
            grd.addColorStop(1, "rgba(120, 190, 255, 0)");
            ctx.fillStyle = grd;
            ctx.beginPath();
            ctx.arc(p.x, p.y, hr, 0, Math.PI * 2);
            ctx.fill();
            ctx.restore();
          }
          ctx.globalCompositeOperation = "source-over";
          if (word && !cnsLabelsOpen && !on) {
            ctx.globalCompositeOperation = "lighter";
            continue;
          }
          let a = word ? Math.min(0.55, zoomA * 0.7) : zoomA;
          if (self.hover === s) a = Math.min(1, a + (word ? 0.28 : 0.4));
          if (self.ctrl) {
            const d = Math.hypot(p.x - self.mouse.x, p.y - self.mouse.y);
            if (d < 150) a = Math.min(1, a + (1 - d / 150) * 0.7);
          }
          ctx.globalAlpha = a;
          ctx.fillStyle = on
            ? word
              ? "rgba(210, 200, 180, 0.88)"
              : "rgba(255, 232, 208, 0.95)"
            : word
              ? "rgba(160, 176, 196, 0.62)"
              : "rgba(198, 214, 235, 0.8)";
          const sz = word
            ? Math.max(8, Math.min(11, 7.8 + self.cam.scale * 0.7))
            : Math.max(8.5, Math.min(11.5, 8.6 + self.cam.scale * 0.85));
          ctx.font = word
            ? "400 " +
              sz +
              'px "Segoe UI", "Microsoft YaHei", "Noto Sans CJK", Bahnschrift, sans-serif'
            : "400 " + sz + "px " + labelFont;
          if (ctx.letterSpacing !== undefined) ctx.letterSpacing = "0.06em";
          ctx.textAlign = "center";
          ctx.textBaseline = "top";
          ctx.shadowColor = word ? "rgba(0,0,0,0.7)" : "rgba(0,0,0,0.92)";
          ctx.shadowBlur = word ? 3 : 7;
          if (self.sel.has(s.name)) {
            const tw = ctx.measureText(s.name).width;
            const bx = p.x - tw / 2 - 5;
            const by = p.y + r + 5;
            ctx.shadowBlur = 0;
            ctx.globalAlpha = 0.92;
            ctx.fillStyle = "rgba(28, 72, 118, 0.88)";
            if (ctx.roundRect) {
              ctx.beginPath();
              ctx.roundRect(bx, by, tw + 10, sz + 5, 4);
              ctx.fill();
              ctx.strokeStyle = "rgba(160, 220, 255, 0.9)";
              ctx.lineWidth = 1;
              ctx.stroke();
            } else {
              ctx.fillRect(bx, by, tw + 10, sz + 5);
            }
            ctx.fillStyle = "rgba(255, 244, 220, 0.98)";
          }
          ctx.fillText(s.name, p.x, p.y + r + 7);
          ctx.globalAlpha = 1;
          ctx.shadowBlur = 0;
          if (ctx.letterSpacing !== undefined) ctx.letterSpacing = "0px";
          ctx.globalCompositeOperation = "lighter";
        }
        ctx.globalCompositeOperation = "source-over";
        kickDraw();
      }

      let drawRaf = 0;
      function kickDraw() {
        if (self.dead) return;
        if (drawRaf) {
          cancelAnimationFrame(drawRaf);
          drawRaf = 0;
        }
        drawRaf = requestAnimationFrame(() => {
          drawRaf = 0;
          draw();
        });
      }

      let flyGen = 0;
      function cancelFly() {
        flyGen += 1;
        self.flying = false;
        self.flyDir = null;
      }
      function fly(to, then) {
        flyGen += 1;
        const gen = flyGen;
        self.flying = true;
        const start = { x: self.cam.x, y: self.cam.y, scale: self.cam.scale };
        const end = to;
        const t0 = performance.now();
        const dur = 320;
        function step(now) {
          if (gen !== flyGen) return;
          const t = Math.min(1, (now - t0) / dur);
          const e = t < 0.5 ? 2 * t * t : -1 + (4 - 2 * t) * t;
          self.cam.x = start.x + (end.x - start.x) * e;
          self.cam.y = start.y + (end.y - start.y) * e;
          self.cam.scale = start.scale + (end.scale - start.scale) * e;
          if (t < 1) requestAnimationFrame(step);
          else {
            self.flying = false;
            self.flyDir = null;
            if (h.onCamera) h.onCamera(self.cam);
            if (then) then();
          }
        }
        requestAnimationFrame(step);
      }

      function flyEnter(star, then) {
        if (!star) {
          if (then) then();
          return;
        }
        fly({ x: star.x, y: star.y, scale: self.cam.scale * 2.3 }, then);
      }

      function flyUp(then) {
        fly({ x: self.cam.x, y: self.cam.y, scale: Math.max(0.25, self.cam.scale * 0.45) }, then);
      }

      function bind(el, type, fn, opts) {
        el.addEventListener(
          type,
          (e) => {
            if (!self.live || self.dead) return;
            fn(e);
          },
          opts
        );
      }

      const SCALE_MIN = 0.18;
      function scaleMax() {
        return self.hostLayout ? 36 : 6;
      }
      let lastWheelAt = 0;
      let zoomOnlyGesture = false;
      let firstFittedOut = true;
      let firstEnter = true;
      function isNav(e) {
        return e.ctrlKey || e.metaKey;
      }
      function zoomCamera(e) {
        const world = toWorld(e.clientX, e.clientY);
        const f = e.deltaY < 0 ? 1.12 : 1 / 1.12;
        self.cam.scale = Math.max(SCALE_MIN, Math.min(scaleMax(), self.cam.scale * f));
        const world2 = toWorld(e.clientX, e.clientY);
        self.cam.x += world.x - world2.x;
        self.cam.y += world.y - world2.y;
        if (h.onCamera) h.onCamera(self.cam);
      }
      function zoomOutMaxed() {
        return self.cam.scale <= SCALE_MIN * 1.001;
      }
      function sceneFits() {
        const stars = self.stars;
        if (!stars.length) return true;
        const w = self.w;
        const hgt = self.h;
        const pad = Math.max(24, 0.12 * Math.min(w, hgt));
        for (let i = 0; i < stars.length; i++) {
          const s = stars[i];
          const p = toScreen(s);
          const r = starR(s);
          if (p.x - r < pad || p.x + r > w - pad) return false;
          if (p.y - r < pad || p.y + r + 18 > hgt - pad) return false;
        }
        return true;
      }
      function wheelUnits(e) {
        const dy = Math.abs(e.deltaY);
        if (e.deltaMode === 1) return dy * 120;
        if (e.deltaMode === 2) return dy * Math.min(self.w, self.h);
        return dy;
      }
      function leaveScene() {
        if (self.flying && self.flyDir === "out") return;
        self.flyDir = "out";
        flyUp(() => h.onUp && h.onUp());
      }
      function enterStar(star) {
        if (!star || star.kind !== "dir") return;
        if (self.flying && self.flyDir === "in") return;
        self.flyDir = "in";
        flyEnter(star, () => h.onEnter && h.onEnter(star.path));
      }
      bind(
        container,
        "wheel",
        (e) => {
          e.preventDefault();
          e.stopPropagation();
          const now = performance.now();
          if (now - lastWheelAt > 350) {
            zoomOnlyGesture = !sceneFits();
            self.leaveAcc = 0;
            firstFittedOut = true;
            self.enterAcc = 0;
            firstEnter = true;
            self.enterTarget = null;
          }
          lastWheelAt = now;
          const s = hit(e.clientX, e.clientY);
          const zoomIn = e.deltaY < 0;
          if (isNav(e)) {
            self.leaveAcc = 0;
            self.enterAcc = 0;
            self.enterTarget = null;
            cancelFly();
            zoomCamera(e);
            return;
          }
          if (zoomIn) {
            self.leaveAcc = 0;
            if (s && s.kind === "dir") {
              const same =
                self.enterTarget &&
                (self.enterTarget === s || self.enterTarget.path === s.path);
              if (!same) {
                self.enterAcc = 0;
                firstEnter = true;
                self.enterTarget = s;
              }
              self.enterAcc += wheelUnits(e);
              const canEnter = !firstEnter && self.enterAcc >= enterDebtNeed(self.w, self.h);
              firstEnter = false;
              if (canEnter) {
                enterStar(s);
                return;
              }
              zoomCamera(e);
              return;
            }
            self.enterAcc = 0;
            self.enterTarget = null;
            firstEnter = true;
            zoomCamera(e);
            return;
          }
          self.enterAcc = 0;
          self.enterTarget = null;
          if (zoomOnlyGesture) {
            cancelFly();
            zoomCamera(e);
            return;
          }
          if (typeof h.scrollLeave === "function" && !h.scrollLeave()) {
            cancelFly();
            zoomCamera(e);
            return;
          }
          if (!sceneFits() && !zoomOutMaxed()) {
            cancelFly();
            zoomCamera(e);
            return;
          }
          self.leaveAcc += wheelUnits(e);
          const need = leaveDebtNeed(self.w, self.h);
          const canLeave = !firstFittedOut && self.leaveAcc >= need;
          firstFittedOut = false;
          if (canLeave) {
            leaveScene();
            return;
          }
          if (!zoomOutMaxed()) {
            cancelFly();
            zoomCamera(e);
          }
        },
        { passive: false, capture: true }
      );

      bind(container, "pointerdown", (e) => {
        if (e.button !== 0 && e.button !== 1) return;
        e.preventDefault();
        if (h.onActivate) h.onActivate();
        try {
          container.setPointerCapture(e.pointerId);
        } catch (_) {}
        const s = e.button === 0 ? hit(e.clientX, e.clientY) : null;
        const r = container.getBoundingClientRect();
        const second =
          e.button === 0 &&
          self.lastClick &&
          performance.now() - self.lastClick.t < 500 &&
          Math.hypot(e.clientX - self.lastClick.x, e.clientY - self.lastClick.y) < 12;
        let kind = "select";
        if (e.button === 1 || (e.button === 0 && e.altKey && !isNav(e))) kind = "pan";
        else if (e.button === 0 && second && !isNav(e) && !s) kind = "dblpan";
        else if (e.button === 0 && isNav(e)) kind = "nav";
        else if (e.button === 0 && s) kind = "star";
        if (kind === "pan" || kind === "dblpan") {
          self.leaveAcc = 0;
          self.enterAcc = 0;
          self.enterTarget = null;
        }
        self.drag = {
          kind,
          star: s,
          lx: e.clientX,
          ly: e.clientY,
          moved: false,
          sx: e.clientX - r.left,
          sy: e.clientY - r.top,
        };
      });

      bind(container, "pointermove", (e) => {
        const r = container.getBoundingClientRect();
        self.mouse.x = e.clientX - r.left;
        self.mouse.y = e.clientY - r.top;
        self.hover = hit(e.clientX, e.clientY);
        if (h.onHover) h.onHover(self.hover);
        const w = toWorld(e.clientX, e.clientY);
        if (h.onWorldPointer) h.onWorldPointer(w, { x: e.clientX, y: e.clientY });
        if (!self.drag) return;
        const dx = e.clientX - self.drag.lx;
        const dy = e.clientY - self.drag.ly;
        if (Math.hypot(dx, dy) > 3) {
          if (self.drag.kind === "dblpan") self.suppressDblClick = true;
          self.drag.moved = true;
          if (self.drag.kind === "dblpan") self.drag.kind = "pan";
          else if (self.drag.kind === "select") self.drag.kind = "marquee";
        }
        if (self.drag.kind === "pan") {
          self.cam.x -= dx / self.cam.scale;
          self.cam.y -= dy / self.cam.scale;
          self.drag.lx = e.clientX;
          self.drag.ly = e.clientY;
          if (h.onCamera) h.onCamera(self.cam);
        } else if (self.drag.kind === "star" && self.drag.star) {
          const st = self.drag.star;
          st.x += dx / self.cam.scale;
          st.y += dy / self.cam.scale;
          if (!self.arranged) self.frozen[st.name] = { x: st.x, y: st.y };
          self.drag.lx = e.clientX;
          self.drag.ly = e.clientY;
        } else if (self.drag.kind === "marquee") {
          setMarquee(self.drag.sx, self.drag.sy, self.mouse.x, self.mouse.y);
        }
      });

      function endDrag(e) {
        if (!self.drag) return;
        const d = self.drag;
        self.drag = null;
        hideMarquee();
        try {
          container.releasePointerCapture(e.pointerId);
        } catch (_) {}
        if (
          e.button === 0 &&
          !d.moved &&
          d.kind !== "dblpan" &&
          d.kind !== "nav" &&
          d.kind !== "pan"
        ) {
          self.lastClick = { t: performance.now(), x: e.clientX, y: e.clientY };
        } else {
          self.lastClick = null;
        }
        if (d.kind === "star" && d.moved) {
          if (!self.arranged) emitPos();
          if (h.onStarDragEnd) h.onStarDragEnd(d.star, e);
          return;
        }
        if (d.kind === "pan") return;
        if (d.kind === "marquee") {
          const r = container.getBoundingClientRect();
          const x1 = e.clientX - r.left;
          const y1 = e.clientY - r.top;
          const minx = Math.min(d.sx, x1);
          const maxx = Math.max(d.sx, x1);
          const miny = Math.min(d.sy, y1);
          const maxy = Math.max(d.sy, y1);
          const hits = self.stars.filter((st) => {
            const p = toScreen(st);
            return p.x >= minx && p.x <= maxx && p.y >= miny && p.y <= maxy;
          });
          const add = e.ctrlKey || e.metaKey || e.shiftKey;
          if (!hits.length) {
            if (!add) {
              self.sel.clear();
              self.anchor = null;
              announceSel();
            }
            return;
          }
          if (add) hits.forEach((st) => self.sel.add(st.name));
          else self.sel = new Set(hits.map((st) => st.name));
          self.anchor = hits[hits.length - 1];
          announceSel();
          return;
        }
        if (d.moved) return;
        if (d.kind === "nav" || isNav(e)) {
          const s = d.star || hit(e.clientX, e.clientY);
          if (s && s.kind === "dir") enterStar(s);
          else if (!s) leaveScene();
          return;
        }
        const s = d.star;
        if (!s) {
          self.sel.clear();
          self.anchor = null;
          announceSel();
          return;
        }
        if (e.shiftKey && self.anchor) {
          self.sel = new Set(spatialRange(self.anchor, s).map((x) => x.name));
        } else {
          self.sel = new Set([s.name]);
          self.anchor = s;
        }
        announceSel();
      }

      bind(container, "pointerup", endDrag);
      bind(container, "pointercancel", () => {
        hideMarquee();
        self.drag = null;
      });

      bind(container, "dblclick", (e) => {
        if (e.button !== 0) return;
        e.preventDefault();
        e.stopPropagation();
        if (self.suppressDblClick) {
          self.suppressDblClick = false;
          return;
        }
        const s = hit(e.clientX, e.clientY);
        if (h.onDblClick) h.onDblClick(e, s || null);
      });

      bind(container, "contextmenu", (e) => {
        e.preventDefault();
        e.stopPropagation();
        const s = hit(e.clientX, e.clientY);
        if (h.onContextMenu) h.onContextMenu(e, s);
      });

      bind(container, "dragover", (e) => {
        e.preventDefault();
        if (e.dataTransfer) e.dataTransfer.dropEffect = e.shiftKey ? "move" : "copy";
      });
      bind(container, "drop", (e) => {
        e.preventDefault();
        if (h.onDrop) h.onDrop(e);
      });

      window.addEventListener("keydown", (e) => {
        if (e.key === "Control") self.ctrl = true;
      });
      window.addEventListener("keyup", (e) => {
        if (e.key === "Control") self.ctrl = false;
      });

      const ro = typeof ResizeObserver === "function" ? new ResizeObserver(() => fit()) : null;
      if (ro) ro.observe(container);
      fit();
      requestAnimationFrame(() => {
        fit();
        requestAnimationFrame(fit);
      });
      setTimeout(fit, 50);
      setTimeout(fit, 200);
      kickDraw();

      return {
        setScene,
        arrangeBy,
        galaxy,
        flyEnter,
        flyUp,
        flyTo(x, y, scale) {
          fly({
            x: x,
            y: y,
            scale: scale != null ? scale : Math.max(1.35, self.cam.scale),
          });
        },
        worldBBox() {
          const r = container.getBoundingClientRect();
          const a = toWorld(r.left, r.top);
          const b = toWorld(r.right, r.bottom);
          return {
            xmin: Math.min(a.x, b.x),
            xmax: Math.max(a.x, b.x),
            ymin: Math.min(a.y, b.y),
            ymax: Math.max(a.y, b.y),
            scale: self.cam.scale,
          };
        },
        getCamera() {
          return Object.assign({}, self.cam);
        },
        setCamera(c) {
          Object.assign(self.cam, c || {});
        },
        getPositions() {
          return cloneMap(self.frozen);
        },
        highlight(name) {
          self.highlight = name;
        },
        highlightParent(parentPath) {
          const s = self.stars.find((x) => x.path === parentPath || x.name === parentPath);
          self.highlightParent = s ? s.name : self.dirPath === parentPath ? "__self__" : null;
          if (self.dirPath === parentPath) {
            self.highlightParent = null;
            self.highlight = null;
          }
        },
        selectNames(names) {
          self.sel = new Set(names || []);
          announceSel();
        },
        getSelection() {
          return [...self.sel].map((n) => self.stars.find((s) => s.name === n)).filter(Boolean);
        },
        getHover() {
          return self.hover;
        },
        stars() {
          return self.stars;
        },
        starAtClient: hit,
        worldFromClient: toWorld,
        screenOf(star) {
          const p = toScreen(star);
          const r = container.getBoundingClientRect();
          return { x: r.left + p.x, y: r.top + p.y, r: starR(star) };
        },
        notePending(name, x, y, kind) {
          if (!self.pending) self.pending = {};
          const p = spiralAway(x, y, hintBlockers(self.stars, self.pending, name), kind);
          self.pending[name] = { x: p.x, y: p.y, kind: kind || "file" };
        },
        placeHint() {
          return { x: self.cam.x, y: self.cam.y };
        },
        resize: fit,
        setLooks(map) {
          self.looks = map || {};
          stampGlyphs();
        },
        setLive(on) {
          self.live = !!on;
          if (self.live) kickDraw();
          if (!self.live) {
            self.drag = null;
            hideMarquee();
          }
        },
        canvas,
        captureDataUrl(opts) {
          const type = (opts && opts.type) || "image/jpeg";
          const quality = opts && opts.quality != null ? opts.quality : 0.72;
          const maxEdge = (opts && opts.maxEdge) || 1280;
          return captureCanvasDataUrl(canvas, type, quality, maxEdge);
        },
        statusEl: status,
        destroy() {
          self.dead = true;
          if (ro) ro.disconnect();
          canvas.remove();
          status.remove();
        },
      };
    },
  };
})();
