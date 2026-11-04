(function () {
  const GOLDEN = 2.399963229728653;

  function starDragGhostEl() {
    let el = document.getElementById("star-drag-ghost");
    if (!el) {
      el = document.createElement("img");
      el.id = "star-drag-ghost";
      el.alt = "";
      el.draggable = false;
      document.body.appendChild(el);
    }
    return el;
  }

  window.SDStarDrag = {
    capture(star, r) {
      const Look = window.SDLook;
      const rad = Math.max(8, r || 12);
      // File paintFileStar bloom/spikes go to ~5.6×r; a tight pad clips to a square.
      const pad = Math.ceil(rad * (star && star.kind === "dir" ? 3.4 : 6.2));
      const c = document.createElement("canvas");
      c.width = pad * 2;
      c.height = pad * 2;
      const ctx = c.getContext("2d");
      const x = pad;
      const y = pad;
      const glyph = star && star.glyph;
      let painted = false;
      if (glyph && glyph.image) {
        const im = new Image();
        im.src = glyph.image;
        if (im.complete && im.naturalWidth) {
          ctx.save();
          ctx.beginPath();
          ctx.arc(x, y, pad * 0.52, 0, Math.PI * 2);
          ctx.clip();
          const side = rad * (star.kind === "dir" ? 3.2 : 2.6);
          ctx.drawImage(im, x - side / 2, y - side / 2, side, side);
          ctx.restore();
          painted = true;
        }
      }
      if (!painted && Look && star) {
        const hsh = Look.hashName(star.name);
        const phase = star.twinkle0 || 0;
        if (star.kind === "dir") {
          Look.paintDirNebula(ctx, x, y, rad, {
            on: true,
            breathe: 1,
            style: (glyph && glyph.dirStyle) || star.dirStyle || Look.dirStyle(star.name),
            rot: phase * 0.4,
            seed: hsh,
          });
        } else {
          const look = Look.starLook(star.name);
          Look.paintFileStar(ctx, x, y, rad, {
            on: true,
            twinkle: 1,
            spec: (glyph && glyph.spec) || look.spec,
            morph: (glyph && glyph.morph) || look.morph,
          });
        }
        painted = true;
      }
      if (!painted) {
        ctx.fillStyle = "rgba(180, 220, 255, 0.9)";
        ctx.beginPath();
        ctx.arc(x, y, rad, 0, Math.PI * 2);
        ctx.fill();
      }
      return { url: c.toDataURL("image/png"), size: pad * 2 };
    },
    show(ghost, cx, cy) {
      if (!ghost) return;
      const el = starDragGhostEl();
      if (el.getAttribute("src") !== ghost.url) el.src = ghost.url;
      el.style.width = ghost.size + "px";
      el.style.height = ghost.size + "px";
      el.style.left = cx + "px";
      el.style.top = cy + "px";
      el.classList.add("on");
    },
    hide() {
      const el = document.getElementById("star-drag-ghost");
      if (el) el.classList.remove("on");
    },
  };

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
    return kind === "dir" ? 60 : 30;
  }

  const GRID_W = 96;
  const GRID_H = 84;

  function fitEndName(ctx, name, maxW) {
    const s = String(name || "");
    if (!s || ctx.measureText(s).width <= maxW) return s;
    const ell = "…";
    if (ctx.measureText(ell).width >= maxW) return ell;
    let lo = 0;
    let hi = s.length;
    while (lo < hi) {
      const mid = ((lo + hi + 1) / 2) | 0;
      if (ctx.measureText(s.slice(0, mid) + ell).width <= maxW) lo = mid;
      else hi = mid - 1;
    }
    return lo > 0 ? s.slice(0, lo) + ell : ell;
  }

  function gridCell(x, y) {
    return { c: Math.round(x / GRID_W), r: Math.round(y / GRID_H) };
  }

  function gridXy(c, r) {
    return { x: c * GRID_W, y: r * GRID_H };
  }

  function occupyKey(c, r) {
    return c + ":" + r;
  }

  function emptyGridCell(x, y, blockers) {
    const taken = {};
    (blockers || []).forEach((o) => {
      const g = gridCell(o.x, o.y);
      taken[occupyKey(g.c, g.r)] = 1;
    });
    const gc = gridCell(x, y);
    const band = 24;
    for (let i = 0; i < 2048; i++) {
      const c = gc.c + (i % band);
      const r = gc.r + ((i / band) | 0);
      if (!taken[occupyKey(c, r)]) return gridXy(c, r);
    }
    return gridXy(gc.c + 1, gc.r);
  }

  function packGrid(children, origin, viewW, viewH, scale) {
    const items = (children || []).slice().sort((a, b) => (a.name || "").localeCompare(b.name || ""));
    const n = items.length;
    const pos = {};
    if (!n) return pos;
    const ox = origin && origin.x != null ? origin.x : 0;
    const oy = origin && origin.y != null ? origin.y : 0;
    const sc = Math.max(0.18, scale || 1);
    const worldW = (viewW || 480) / sc;
    const worldH = (viewH || 320) / sc;
    const maxCols = Math.max(1, Math.floor((worldW * 0.84) / GRID_W));
    const maxRows = Math.max(1, Math.floor((worldH * 0.84) / GRID_H));
    let cols = Math.max(1, Math.round(Math.sqrt(n)));
    cols = Math.min(n, cols, maxCols);
    let rows = Math.ceil(n / cols);
    if (rows > maxRows) {
      cols = Math.min(n, maxCols, Math.max(1, Math.ceil(n / maxRows)));
      rows = Math.ceil(n / cols);
    }
    const x0 = Math.round((ox - ((cols - 1) * GRID_W) / 2) / GRID_W) * GRID_W;
    const y0 = Math.round((oy - ((rows - 1) * GRID_H) / 2) / GRID_H) * GRID_H;
    items.forEach((s, i) => {
      pos[s.name] = { x: x0 + (i % cols) * GRID_W, y: y0 + Math.floor(i / cols) * GRID_H };
    });
    return pos;
  }

  function placeUnoccupiedGrid(missing, saved, origin) {
    const blockers = [];
    Object.keys(saved || {}).forEach((k) => {
      const p = saved[k];
      if (!p) return;
      blockers.push({ x: p.x, y: p.y });
    });
    const ox = origin && origin.x != null ? origin.x : 0;
    const oy = origin && origin.y != null ? origin.y : 0;
    const pos = {};
    (missing || []).forEach((s) => {
      const p = emptyGridCell(ox, oy, blockers);
      pos[s.name] = { x: p.x, y: p.y };
      blockers.push({ x: p.x, y: p.y });
    });
    return pos;
  }

  function placeUnoccupied(missing, saved, origin) {
    const blockers = [];
    Object.keys(saved || {}).forEach((k) => {
      const p = saved[k];
      if (!p) return;
      blockers.push({ x: p.x, y: p.y, kind: p.kind || "file" });
    });
    const ox = origin && origin.x != null ? origin.x : 0;
    const oy = origin && origin.y != null ? origin.y : 0;
    const pos = {};
    (missing || []).forEach((s) => {
      const p = spiralAway(ox, oy, blockers, s.kind);
      pos[s.name] = { x: p.x, y: p.y };
      blockers.push({ x: p.x, y: p.y, kind: s.kind || "file" });
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

  function clusterStats(blockers) {
    if (!blockers || !blockers.length) return null;
    let sx = 0;
    let sy = 0;
    let minx = Infinity;
    let maxx = -Infinity;
    let miny = Infinity;
    let maxy = -Infinity;
    blockers.forEach((o) => {
      sx += o.x;
      sy += o.y;
      if (o.x < minx) minx = o.x;
      if (o.x > maxx) maxx = o.x;
      if (o.y < miny) miny = o.y;
      if (o.y > maxy) maxy = o.y;
    });
    const n = blockers.length;
    const cx = sx / n;
    const cy = sy / n;
    let r = 0;
    blockers.forEach((o) => {
      const d = Math.hypot(o.x - cx, o.y - cy);
      if (d > r) r = d;
    });
    return { cx, cy, r, minx, maxx, miny, maxy };
  }

  function convexHull(pts) {
    if (!pts || pts.length < 3) return null;
    const raw = pts
      .map((o) => ({ x: o.x, y: o.y }))
      .sort((a, b) => a.x - b.x || a.y - b.y);
    const uniq = [];
    raw.forEach((q) => {
      const last = uniq[uniq.length - 1];
      if (!last || last.x !== q.x || last.y !== q.y) uniq.push(q);
    });
    if (uniq.length < 3) return null;
    const cross = (o, a, b) => (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x);
    const lower = [];
    uniq.forEach((q) => {
      while (lower.length >= 2 && cross(lower[lower.length - 2], lower[lower.length - 1], q) <= 0)
        lower.pop();
      lower.push(q);
    });
    const upper = [];
    for (let i = uniq.length - 1; i >= 0; i--) {
      const q = uniq[i];
      while (upper.length >= 2 && cross(upper[upper.length - 2], upper[upper.length - 1], q) <= 0)
        upper.pop();
      upper.push(q);
    }
    lower.pop();
    upper.pop();
    return lower.concat(upper);
  }

  function pointInHull(hull, x, y) {
    if (!hull || hull.length < 3) return false;
    let inside = false;
    for (let i = 0, j = hull.length - 1; i < hull.length; j = i++) {
      const xi = hull[i].x;
      const yi = hull[i].y;
      const xj = hull[j].x;
      const yj = hull[j].y;
      const denom = yj - yi || 1e-12;
      const hit = yi > y !== yj > y && x < ((xj - xi) * (y - yi)) / denom + xi;
      if (hit) inside = !inside;
    }
    return inside;
  }

  function distPointSeg(px, py, ax, ay, bx, by) {
    const dx = bx - ax;
    const dy = by - ay;
    const len2 = dx * dx + dy * dy;
    if (len2 < 1e-8) return Math.hypot(px - ax, py - ay);
    let t = ((px - ax) * dx + (py - ay) * dy) / len2;
    if (t < 0) t = 0;
    else if (t > 1) t = 1;
    return Math.hypot(px - (ax + dx * t), py - (ay + dy * t));
  }

  function knnPairs(blockers, k) {
    const n = blockers.length;
    const seen = {};
    const pairs = [];
    for (let i = 0; i < n; i++) {
      const ds = [];
      for (let j = 0; j < n; j++) {
        if (i === j) continue;
        ds.push({ j, d: Math.hypot(blockers[i].x - blockers[j].x, blockers[i].y - blockers[j].y) });
      }
      ds.sort((a, b) => a.d - b.d);
      const take = Math.min(k, ds.length);
      for (let t = 0; t < take; t++) {
        const j = ds[t].j;
        const a = i < j ? i : j;
        const b = i < j ? j : i;
        const key = a + ":" + b;
        if (seen[key]) continue;
        seen[key] = 1;
        pairs.push([a, b, ds[t].d]);
      }
    }
    return pairs;
  }

  function pickRand(list) {
    if (!list || !list.length) return null;
    return list[(Math.random() * list.length) | 0];
  }

  function samePt(a, b) {
    return Math.abs(a.x - b.x) < 0.01 && Math.abs(a.y - b.y) < 0.01;
  }

  function isHullEdge(hull, a, b) {
    if (!hull || hull.length < 2) return false;
    for (let i = 0, j = hull.length - 1; i < hull.length; j = i++) {
      const p = hull[j];
      const q = hull[i];
      if ((samePt(p, a) && samePt(q, b)) || (samePt(p, b) && samePt(q, a))) return true;
    }
    return false;
  }

  function spiralAway(x, y, blockers, kind) {
    const selfSep = starSep(kind || "dir");
    const needOf = (o) => selfSep + starSep(o.kind);
    const blocked = (px, py) => {
      for (let i = 0; i < blockers.length; i++) {
        if (Math.hypot(px - blockers[i].x, py - blockers[i].y) < needOf(blockers[i])) return true;
      }
      return false;
    };
    if (!blockers || !blockers.length) return { x, y };
    const box = clusterStats(blockers);
    const hull = convexHull(blockers);
    const inGap = (px, py) => {
      if (blocked(px, py)) return false;
      if (hull && !pointInHull(hull, px, py)) return false;
      return true;
    };
    const pad = selfSep * 1.15;
    const corridors = [];
    const pairs = knnPairs(blockers, 6);
    for (let p = 0; p < pairs.length; p++) {
      const a = blockers[pairs[p][0]];
      const b = blockers[pairs[p][1]];
      const d = pairs[p][2];
      if (d < selfSep * 2) continue;
      if (isHullEdge(hull, a, b)) continue;
      let clear = true;
      for (let i = 0; i < blockers.length; i++) {
        if (blockers[i] === a || blockers[i] === b) continue;
        if (distPointSeg(blockers[i].x, blockers[i].y, a.x, a.y, b.x, b.y) < pad) {
          clear = false;
          break;
        }
      }
      if (!clear) continue;
      const ux = (b.x - a.x) / d;
      const uy = (b.y - a.y) / d;
      const rx = -uy;
      const ry = ux;
      const spots = [];
      const steps = Math.max(4, Math.min(9, Math.round(d / (selfSep * 1.4))));
      for (let s = 1; s < steps; s++) {
        const t = s / steps;
        if (t < 0.22 || t > 0.78) continue;
        const px = a.x + ux * d * t;
        const py = a.y + uy * d * t;
        if (inGap(px, py)) spots.push({ x: px, y: py });
      }
      const midX = (a.x + b.x) / 2;
      const midY = (a.y + b.y) / 2;
      const off = selfSep * 0.8;
      const p1 = { x: midX + rx * off, y: midY + ry * off };
      const p2 = { x: midX - rx * off, y: midY - ry * off };
      if (inGap(p1.x, p1.y)) spots.push(p1);
      if (inGap(p2.x, p2.y)) spots.push(p2);
      if (spots.length) corridors.push({ d, spots });
    }
    if (corridors.length) {
      let maxD = 0;
      for (let i = 0; i < corridors.length; i++) {
        if (corridors[i].d > maxD) maxD = corridors[i].d;
      }
      const large = corridors.filter((c) => c.d >= maxD * 0.72);
      const chosen = pickRand(large);
      const spot = pickRand(chosen.spots);
      if (spot) return spot;
    }

    const N = 16;
    const rim = [];
    for (let i = 0; i < N; i++) {
      const a = (i * Math.PI * 2) / N;
      const ux = Math.cos(a);
      const uy = Math.sin(a);
      let support = selfSep;
      for (let b = 0; b < blockers.length; b++) {
        const o = blockers[b];
        const s = (o.x - box.cx) * ux + (o.y - box.cy) * uy + needOf(o);
        if (s > support) support = s;
      }
      for (let t = 0; t < 12; t++) {
        const rr = support + t * (selfSep * 0.18);
        const px = box.cx + ux * rr;
        const py = box.cy + uy * rr;
        if (!blocked(px, py)) {
          rim.push({ x: px, y: py });
          break;
        }
      }
    }
    const edge = pickRand(rim);
    if (edge) return edge;
    if (!blocked(x, y)) return { x, y };
    return { x: box.cx + box.r + selfSep * 2, y: box.cy };
  }

  function cloneMap(m) {
    const o = {};
    Object.keys(m || {}).forEach((k) => {
      const p = m[k];
      if (p) o[k] = { x: p.x, y: p.y };
    });
    return o;
  }

  function livePosMap(map, names) {
    const out = {};
    Object.keys(map || {}).forEach((k) => {
      if (names.has(k) && map[k]) out[k] = map[k];
    });
    return out;
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
        layoutMode: "galaxy",
        sel: new Set(),
        searchSel: new Set(),
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
        const look = looks[lookKey(star.path)] || looks[star.path] || null;
        if (look && look.image) return look;
        if (star && star.logo) return { image: star.logo };
        return look;
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
        self.layoutMode = opts.layoutMode === "grid" ? "grid" : "galaxy";
        self.dirPath = opts.dirPath || "";
        if (opts.resetCamera) self.cam = { x: 0, y: 0, scale: 1 };
        else if (opts.camera) self.cam = Object.assign({}, self.cam, opts.camera);
        fit();
        const children = (opts.children || []).map((c) => Object.assign({}, c));
        const saved = cloneMap(opts.savedPositions || {});
        const missing = children.filter((c) => !saved[c.name]);
        let map = saved;
        let changed = false;
        const origin = { x: self.cam.x, y: self.cam.y };
        if (opts.hostLayout) {
          map = {};
          children.forEach((c) => {
            map[c.name] = { x: c.x || 0, y: c.y || 0 };
          });
          changed = false;
        } else if (!Object.keys(saved).length && children.length) {
          map =
            self.layoutMode === "grid"
              ? packGrid(children, origin, self.w, self.h, self.cam.scale)
              : layoutGalaxy(children);
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
          const placed =
            rest.length === 0
              ? {}
              : self.layoutMode === "grid"
                ? placeUnoccupiedGrid(rest, occ, origin)
                : placeUnoccupied(rest, occ, origin);
          map = Object.assign({}, saved, placed);
          changed = true;
        }
        const pend = self.pending || {};
        const names = new Set(children.map((c) => c.name));
        if (!opts.hostLayout) {
          Object.keys(pend).forEach((name) => {
            if (!names.has(name) || saved[name]) return;
            map[name] = { x: pend[name].x, y: pend[name].y };
            changed = true;
          });
        }
        if (!opts.hostLayout && !opts.error) {
          const live = livePosMap(map, names);
          if (Object.keys(map).some((k) => !names.has(k))) {
            map = live;
            changed = true;
          }
        }
        const kept = {};
        Object.keys(pend).forEach((name) => {
          if (!names.has(name)) kept[name] = pend[name];
        });
        self.pending = kept;
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
      }

      function arrangeBy(key) {
        const items = self.stars.slice();
        if (key === "name") items.sort((a, b) => a.name.localeCompare(b.name));
        else if (key === "type")
          items.sort((a, b) => a.kind.localeCompare(b.kind) || a.name.localeCompare(b.name));
        else if (key === "size") items.sort((a, b) => (b.size || 0) - (a.size || 0));
        const n = items.length;
        if (!n) return;
        const cellW = 96;
        const cellH = 84;
        const scale = Math.max(0.18, self.cam.scale || 1);
        const fitCols = Math.max(1, Math.floor((((self.w || 480) / scale) * 0.84) / cellW));
        const cols = Math.max(1, Math.min(n, fitCols));
        const rows = Math.ceil(n / cols);
        const x0 = self.cam.x - ((cols - 1) * cellW) / 2;
        const y0 = self.cam.y - ((rows - 1) * cellH) / 2;
        items.forEach((s, i) => {
          s.x = x0 + (i % cols) * cellW;
          s.y = y0 + Math.floor(i / cols) * cellH;
        });
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

      function applyModClick(s, e) {
        if (!s) return;
        if (e.shiftKey && self.anchor) {
          spatialRange(self.anchor, s).forEach((st) => self.sel.add(st.name));
        } else if (self.sel.has(s.name)) {
          self.sel.delete(s.name);
        } else {
          self.sel.add(s.name);
        }
        self.anchor = s;
        announceSel();
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
          if (self.drag && self.drag.kind === "star" && self.drag.moved && self.drag.ghost) {
            const g = self.drag.group;
            if (self.drag.star === s || (g && g.some((x) => x.star === s))) continue;
          }
          const p = toScreen(s);
          if (p.x < -50 || p.y < -50 || p.x > w + 50 || p.y > hgt + 50) continue;
          const r = starR(s);
          const picked = self.sel.has(s.name) || self.searchSel.has(s.name);
          const on =
            self.hover === s ||
            self.highlight === s.name ||
            self.highlightParent === s.name ||
            picked;
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
          if (picked) {
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
          const showFull = on || (self.hover === s);
          const maxW = Math.max(36, Math.min(220, GRID_W * Math.max(0.45, self.cam.scale || 1)));
          const label = word || showFull ? s.name : fitEndName(ctx, s.name, maxW);
          if (picked) {
            const tw = ctx.measureText(label).width;
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
          ctx.fillText(label, p.x, p.y + r + 7);
          ctx.globalAlpha = 1;
          ctx.shadowBlur = 0;
          if (ctx.letterSpacing !== undefined) ctx.letterSpacing = "0px";
          ctx.globalCompositeOperation = "lighter";
        }
        ctx.globalCompositeOperation = "source-over";
        if (self.live && !(window.SD && window.SD.motionFrozen)) kickDraw();
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
      function grabGroup(star) {
        if (!star) return [];
        const names = self.sel.has(star.name) ? [...self.sel] : [star.name];
        return names
          .map((n) => self.stars.find((x) => x.name === n))
          .filter(Boolean)
          .map((st) => ({ star: st, x: st.x, y: st.y }));
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
          px: e.clientX,
          py: e.clientY,
          grabX: s ? s.x : 0,
          grabY: s ? s.y : 0,
          group: s ? grabGroup(s) : [],
          lastClient: { x: e.clientX, y: e.clientY },
          ghost: null,
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
        } else if (self.drag.kind === "star") {
          moveStarFromGrab(e);
        } else if (self.drag.kind === "marquee") {
          setMarquee(self.drag.sx, self.drag.sy, self.mouse.x, self.mouse.y);
        }
      });

      function pointerInContainer(cx, cy) {
        const r = container.getBoundingClientRect();
        return cx >= r.left && cx <= r.right && cy >= r.top && cy <= r.bottom;
      }

      function eventAt(e, d) {
        const fallback = d && d.lastClient;
        const invalid = !e || ((e.clientX === 0 && e.clientY === 0) && fallback);
        return {
          clientX: invalid ? fallback.x : e.clientX,
          clientY: invalid ? fallback.y : e.clientY,
          button: e && typeof e.button === "number" ? e.button : 0,
          pointerId: e && e.pointerId,
          ctrlKey: !!(e && e.ctrlKey),
          metaKey: !!(e && e.metaKey),
          shiftKey: !!(e && e.shiftKey),
        };
      }

      function hideStarGhost() {
        if (window.SDStarDrag) window.SDStarDrag.hide();
      }

      function snapMovedToGrid(d) {
        const moved = (d.group && d.group.length ? d.group : [{ star: d.star }])
          .map((g) => g && g.star)
          .filter(Boolean);
        if (!moved.length) return;
        const taken = {};
        self.stars.forEach((s) => {
          if (moved.indexOf(s) >= 0) return;
          const g = gridCell(s.x, s.y);
          taken[occupyKey(g.c, g.r)] = 1;
        });
        function blockersOf() {
          return Object.keys(taken).map((k) => {
            const parts = k.split(":");
            return gridXy(+parts[0], +parts[1]);
          });
        }
        const leader = d.star || moved[0];
        const from = gridCell(leader.x, leader.y);
        const lp = emptyGridCell(leader.x, leader.y, blockersOf());
        const to = gridCell(lp.x, lp.y);
        const dc = to.c - from.c;
        const dr = to.r - from.r;
        moved.forEach((st) => {
          const g = gridCell(st.x, st.y);
          const p = emptyGridCell((g.c + dc) * GRID_W, (g.r + dr) * GRID_H, blockersOf());
          st.x = p.x;
          st.y = p.y;
          taken[occupyKey(gridCell(p.x, p.y).c, gridCell(p.x, p.y).r)] = 1;
          self.frozen[st.name] = { x: st.x, y: st.y };
        });
      }

      function moveStarFromGrab(e) {
        const d = self.drag;
        if (!d || d.kind !== "star" || !d.star) return;
        d.lastClient = { x: e.clientX, y: e.clientY };
        if (Math.hypot(e.clientX - d.px, e.clientY - d.py) > 3) d.moved = true;
        if (!d.moved) return;
        if (pointerInContainer(e.clientX, e.clientY)) {
          const dx = (e.clientX - d.px) / self.cam.scale;
          const dy = (e.clientY - d.py) / self.cam.scale;
          const members = d.group && d.group.length ? d.group : [{ star: d.star, x: d.grabX, y: d.grabY }];
          members.forEach((g) => {
            const st = g.star;
            if (!st) return;
            st.x = g.x + dx;
            st.y = g.y + dy;
            self.frozen[st.name] = { x: st.x, y: st.y };
          });
          hideStarGhost();
        } else if (window.SDStarDrag) {
          if (!d.ghost) d.ghost = window.SDStarDrag.capture(d.star, starR(d.star));
          window.SDStarDrag.show(d.ghost, e.clientX, e.clientY);
        }
      }

      function endDrag(e) {
        if (!self.drag) return;
        const d = self.drag;
        const ev = eventAt(e, d);
        self.drag = null;
        hideMarquee();
        hideStarGhost();
        try {
          container.releasePointerCapture(ev.pointerId);
        } catch (_) {}
        if (
          ev.button === 0 &&
          !d.moved &&
          d.kind !== "dblpan" &&
          d.kind !== "nav" &&
          d.kind !== "pan"
        ) {
          self.lastClick = { t: performance.now(), x: ev.clientX, y: ev.clientY };
        } else {
          self.lastClick = null;
        }
        if (d.kind === "star" && d.moved) {
          const fileMove = h.onStarDragEnd ? h.onStarDragEnd(d.star, ev) : false;
          if (!fileMove) {
            if (self.layoutMode === "grid") snapMovedToGrid(d);
            emitPos();
            kickDraw();
          }
          return;
        }
        if (d.kind === "pan") return;
        if (d.kind === "marquee") {
          const r = container.getBoundingClientRect();
          const x1 = ev.clientX - r.left;
          const y1 = ev.clientY - r.top;
          const minx = Math.min(d.sx, x1);
          const maxx = Math.max(d.sx, x1);
          const miny = Math.min(d.sy, y1);
          const maxy = Math.max(d.sy, y1);
          const hits = self.stars.filter((st) => {
            const p = toScreen(st);
            return p.x >= minx && p.x <= maxx && p.y >= miny && p.y <= maxy;
          });
          const add = ev.ctrlKey || ev.metaKey || ev.shiftKey;
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
        if (d.kind === "nav" || isNav(ev)) {
          applyModClick(d.star || hit(ev.clientX, ev.clientY), ev);
          return;
        }
        const s = d.star;
        if (!s) {
          self.sel.clear();
          self.anchor = null;
          announceSel();
          return;
        }
        if (ev.shiftKey && self.anchor) {
          self.sel = new Set(spatialRange(self.anchor, s).map((x) => x.name));
        } else {
          self.sel = new Set([s.name]);
          self.anchor = s;
        }
        announceSel();
      }

      bind(container, "pointerup", endDrag);
      bind(container, "pointercancel", (e) => {
        if (self.drag && self.drag.kind === "star") endDrag(e);
        else {
          hideMarquee();
          hideStarGhost();
          self.drag = null;
        }
      });
      bind(window, "pointermove", (e) => {
        if (self.drag && self.drag.kind === "star") moveStarFromGrab(e);
      });
      bind(window, "pointerup", (e) => {
        if (self.drag) endDrag(e);
      });
      bind(window, "pointercancel", (e) => {
        if (self.drag && self.drag.kind === "star") endDrag(e);
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
        previewSelection(names) {
          self.searchSel = new Set(names || []);
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
          const blockers = hintBlockers(self.stars, self.pending, name);
          const p =
            self.layoutMode === "grid"
              ? emptyGridCell(x, y, blockers)
              : spiralAway(x, y, blockers, kind);
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
            hideStarGhost();
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
