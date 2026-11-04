(function () {
  const GOLDEN = 2.399963229728653;
  const THREE_SRC = "https://cdn.jsdelivr.net/npm/three@0.160.0/build/three.min.js";

  function paneDebt(k, w, h) {
    const short = Math.max(96, Math.min(w, h));
    return k * (720 / short);
  }
  function leaveDebtNeed(w, h) { return paneDebt(140, w, h); }  // was 180
  function enterDebtNeed(w, h) { return paneDebt(100, w, h); }  // new

  let loadP = null;

  function loadScript(src) {
    return new Promise((resolve, reject) => {
      const existing = document.querySelector('script[data-sd-three]');
      if (existing) {
        if (window.THREE) return resolve();
        existing.addEventListener("load", () => resolve());
        existing.addEventListener("error", () => reject(new Error("THREE script error")));
        return;
      }
      const s = document.createElement("script");
      s.src = src;
      s.async = true;
      s.dataset.sdThree = "1";
      s.onload = () => resolve();
      s.onerror = () => reject(new Error("Three.js failed to load from CDN"));
      document.head.appendChild(s);
    });
  }

  function ensureThree() {
    if (window.THREE && window.THREE.Scene) return Promise.resolve(window.THREE);
    if (loadP) return loadP;
    loadP = loadScript(THREE_SRC).then(() => {
      if (!window.THREE || !window.THREE.Scene) throw new Error("THREE missing");
      return window.THREE;
    });
    return loadP;
  }

  function layoutGalaxy(items) {
    const dirs = items.filter((s) => s.kind === "dir");
    const files = items.filter((s) => s.kind !== "dir");
    const pos = {};
    function ring(list, r0, spread, ox, z) {
      list.forEach((s, i) => {
        const a = i * GOLDEN;
        const r = r0 + spread * Math.sqrt(i + 1);
        pos[s.name] = { x: ox + Math.cos(a) * r, y: Math.sin(a) * r * 0.85, z };
      });
    }
    ring(dirs, 70, 32, -30, 16);
    ring(files, 150, 36, 40, 0);
    return pos;
  }

  function starSep(kind) {
    return kind === "dir" ? 78 : 39;
  }

  const GRID_W = 56;
  const GRID_H = 48;

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
      const r = gc.r + Math.floor(i / band);
      if (!taken[occupyKey(c, r)]) return gridXy(c, r);
    }
    return gridXy(gc.c + 1, gc.r);
  }

  function packGrid(children, origin, worldW, worldH) {
    const items = (children || []).slice().sort((a, b) => (a.name || "").localeCompare(b.name || ""));
    const n = items.length;
    const pos = {};
    if (!n) return pos;
    const ox = origin && origin.x != null ? origin.x : 0;
    const oy = origin && origin.y != null ? origin.y : 0;
    const maxCols = Math.max(1, Math.floor(((worldW || 480) * 0.84) / GRID_W));
    const maxRows = Math.max(1, Math.floor(((worldH || 320) * 0.84) / GRID_H));
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
      pos[s.name] = {
        x: x0 + (i % cols) * GRID_W,
        y: y0 + Math.floor(i / cols) * GRID_H,
        z: s.kind === "dir" ? 12 : 0,
      };
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
      pos[s.name] = { x: p.x, y: p.y, z: s.kind === "dir" ? 12 : 0 };
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
      pos[s.name] = { x: p.x, y: p.y, z: p.z || 0 };
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
    const z0 = kind === "dir" ? 10 : 0;
    const needOf = (o) => selfSep + starSep(o.kind);
    const blocked = (px, py) => {
      for (let i = 0; i < blockers.length; i++) {
        if (Math.hypot(px - blockers[i].x, py - blockers[i].y) < needOf(blockers[i])) return true;
      }
      return false;
    };
    if (!blockers || !blockers.length) return { x, y, z: z0 };
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
        if (inGap(px, py)) spots.push({ x: px, y: py, z: z0 });
      }
      const midX = (a.x + b.x) / 2;
      const midY = (a.y + b.y) / 2;
      const off = selfSep * 0.8;
      const p1 = { x: midX + rx * off, y: midY + ry * off, z: z0 };
      const p2 = { x: midX - rx * off, y: midY - ry * off, z: z0 };
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
          rim.push({ x: px, y: py, z: z0 });
          break;
        }
      }
    }
    const edge = pickRand(rim);
    if (edge) return edge;
    if (!blocked(x, y)) return { x, y, z: z0 };
    return { x: box.cx + box.r + selfSep * 2, y: box.cy, z: z0 };
  }

  function cloneMap(m) {
    const o = {};
    Object.keys(m || {}).forEach((k) => {
      const p = m[k];
      if (p) o[k] = { x: p.x, y: p.y, z: p.z || 0 };
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

  function dummy(container) {
    const msg = document.createElement("div");
    msg.className = "c3d-missing";
    msg.textContent =
      "Three.js failed to load from the CDN. Toggle back to 2D, or check the network and retry.";
    container.appendChild(msg);
    const pending = {};
    let layoutMode = "galaxy";
    return {
      missing: true,
      setScene(opts) {
        if (!opts || opts.loading) return;
        layoutMode = opts.layoutMode === "grid" ? "grid" : "galaxy";
        const names = new Set((opts.children || []).map((c) => c && c.name));
        Object.keys(pending).forEach((name) => {
          if (names.has(name)) delete pending[name];
        });
      },
      arrangeBy() {},
      galaxy() {},
      flyEnter(_s, then) {
        if (then) then();
      },
      flyUp(then) {
        if (then) then();
      },
      getCamera() {
        return { theta: 0.55, phi: 1.15, r: 420, tx: 0, ty: 0, tz: 0 };
      },
      setCamera() {},
      getPositions() {
        return {};
      },
      highlight() {},
      highlightParent() {},
      previewSelection() {},
      selectNames() {},
      getSelection() {
        return [];
      },
      getHover() {
        return null;
      },
      stars() {
        return [];
      },
      starAtClient() {
        return null;
      },
      worldFromClient() {
        return { x: 0, y: 0 };
      },
      screenOf() {
        return { x: 0, y: 0, r: 12 };
      },
      notePending(name, x, y, kind) {
        const blockers = hintBlockers([], pending, name);
        const p =
          layoutMode === "grid"
            ? emptyGridCell(x, y, blockers)
            : spiralAway(x, y, blockers, kind);
        pending[name] = { x: p.x, y: p.y, z: p.z || (kind === "dir" ? 10 : 0), kind: kind || "file" };
      },
      placeHint() {
        return { x: 0, y: 0 };
      },
      resize() {},
      setLooks() {},
      setLive() {},
      captureDataUrl() {
        return "";
      },
      canvas: null,
      domElement: null,
      statusEl: msg,
      destroy() {
        msg.remove();
      },
    };
  }

  const spriteTex = {};
  const imageTex = {};

  function lookKey(p) {
    return String(p || "")
      .replace(/\//g, "\\")
      .replace(/\\+$/, "")
      .toLowerCase();
  }
  function glyphOf(star, looks) {
    const map = looks || {};
    const look = (star && (map[lookKey(star.path)] || map[star.path])) || null;
    if (look && look.image) return look;
    if (star && star.logo) return { image: star.logo };
    return look;
  }

  function isCnsWord(star) {
    return !!(star && (star.role === "word" || String(star.path || "").indexOf("cns:word:") === 0));
  }

  function getImageTex(THREE, dataUrl) {
    if (imageTex[dataUrl]) return imageTex[dataUrl];
    const img = new Image();
    const tex = new THREE.Texture(img);
    tex.minFilter = THREE.LinearFilter;
    tex.magFilter = THREE.LinearFilter;
    img.onload = () => {
      tex.needsUpdate = true;
    };
    img.src = dataUrl;
    imageTex[dataUrl] = tex;
    return tex;
  }

  function getSpriteTex(THREE, kind, style, extra) {
    const extraKey = extra ? JSON.stringify(extra) : "";
    const key = (kind === "dir" ? "d:" : "f:") + (style || "") + extraKey;
    if (spriteTex[key]) return spriteTex[key];
    const Look = window.SDLook;
    const c = Look
      ? Look.spriteCanvas(kind, style, Object.assign({ seed: Look.hashName(style || "emission"), on: true }, extra || {}))
      : document.createElement("canvas");
    if (!Look) {
      c.width = 256;
      c.height = 256;
    }
    const tex = new THREE.CanvasTexture(c);
    tex.needsUpdate = true;
    spriteTex[key] = tex;
    return tex;
  }

  function makeSprite(THREE, star) {
    const Look = window.SDLook;
    const glyph = star.glyph;
    const isDir = star.kind === "dir";
    if (glyph && glyph.image && !isCnsWord(star)) {
      const mat = new THREE.SpriteMaterial({
        map: getImageTex(THREE, glyph.image),
        color: 0xffffff,
        transparent: true,
        depthWrite: false,
        blending: THREE.NormalBlending,
        opacity: 1,
      });
      const spr = new THREE.Sprite(mat);
      const sc = star.kind === "dir" ? 72 : isCnsWord(star) ? 6 : 36;
      spr.scale.set(sc, sc, 1);
      spr.userData.baseScale = sc;
      return spr;
    }
    const style = isDir
      ? (glyph && glyph.dirStyle) || star.dirStyle || (Look && Look.dirStyle(star.name)) || "emission"
      : "a";
    const baked = !isDir && Look ? Look.starLook(star.name) : { spec: "a", morph: "spike4" };
    const look = {
      spec: (glyph && glyph.spec) || baked.spec,
      morph: (glyph && glyph.morph) || baked.morph,
    };
    const tex = isDir
      ? getSpriteTex(THREE, "dir", style)
      : getSpriteTex(THREE, "file", look.spec, { spec: look.spec, morph: look.morph });
    const mat = new THREE.SpriteMaterial({
      map: tex,
      color: 0xffffff,
      transparent: true,
      depthWrite: false,
      blending: THREE.AdditiveBlending,
      opacity: 1,
    });
    const spr = new THREE.Sprite(mat);
    const sc = isDir ? 72 : isCnsWord(star) ? 6 : 36;
    spr.scale.set(sc, sc, 1);
    spr.userData.baseScale = sc;
    return spr;
  }

  window.Cosmos3D = {
    ensure: ensureThree,
    create(container, h) {
      const THREE = window.THREE;
      if (!THREE || !THREE.Scene) return dummy(container);

      const host = document.createElement("div");
      host.className = "c3d";
      const farCanvas = document.createElement("canvas");
      farCanvas.className = "c3d-far";
      const farCtx = farCanvas.getContext("2d");
      const labels = document.createElement("div");
      labels.className = "c3d-labels";
      const status = document.createElement("div");
      status.className = "vload";
      status.textContent = "Loading…";
      container.appendChild(host);
      container.appendChild(labels);
      container.appendChild(status);
      const marqueeEl = document.createElement("div");
      marqueeEl.className = "marquee-rect";
      marqueeEl.hidden = true;
      container.appendChild(marqueeEl);

      const scene = new THREE.Scene();
      scene.fog = new THREE.FogExp2(0x05070d, 0.0004);
      const camera = new THREE.PerspectiveCamera(55, 1, 1, 8000);
      const renderer = new THREE.WebGLRenderer({ antialias: true, alpha: true });
      renderer.setPixelRatio(Math.min(2, window.devicePixelRatio || 1));
      renderer.setClearColor(0x05070d, 0.04);
      farCanvas.style.zIndex = "0";
      renderer.domElement.style.zIndex = "1";
      host.appendChild(farCanvas);
      host.appendChild(renderer.domElement);

      scene.add(new THREE.AmbientLight(0xffffff, 1));

      const orbit = { theta: 0.55, phi: 1.15, r: 420, tx: 0, ty: 0, tz: 0 };
      const sprites = [];
      const self = {
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
        pending: {},
        flying: false,
        flyDir: null,
        dead: false,
        drag: null,
        ctrl: false,
        mouse: { x: 0, y: 0 },
        loading: true,
        error: "",
        w: 1,
        h: 1,
        leaveAcc: 0,
        enterAcc: 0,
        enterTarget: null,
        lastClick: null,
        suppressDblClick: false,
        farSeed: "",
        far: [],
        live: false,
        looks: {},
        hostLayout: false,
      };

      function applyCam() {
        const t = new THREE.Vector3(orbit.tx, orbit.ty, orbit.tz);
        camera.position.set(
          t.x + orbit.r * Math.sin(orbit.phi) * Math.cos(orbit.theta),
          t.y + orbit.r * Math.cos(orbit.phi),
          t.z + orbit.r * Math.sin(orbit.phi) * Math.sin(orbit.theta)
        );
        camera.lookAt(t);
      }

      function fit() {
        const r = container.getBoundingClientRect();
        const w = Math.max(1, r.width);
        const ht = Math.max(1, r.height);
        camera.aspect = w / ht;
        camera.updateProjectionMatrix();
        renderer.setSize(w, ht, false);
        const dpr = Math.min(2, window.devicePixelRatio || 1);
        farCanvas.width = Math.floor(w * dpr);
        farCanvas.height = Math.floor(ht * dpr);
        farCtx.setTransform(dpr, 0, 0, dpr, 0, 0);
        self.w = w;
        self.h = ht;
      }

      function project(v3) {
        const v = v3.clone().project(camera);
        return {
          x: (v.x * 0.5 + 0.5) * self.w,
          y: (-v.y * 0.5 + 0.5) * self.h,
          vis: v.z > -1 && v.z < 1,
        };
      }

      function pointerWorld(cx, cy) {
        const r = renderer.domElement.getBoundingClientRect();
        const nx = ((cx - r.left) / Math.max(1, r.width)) * 2 - 1;
        const ny = -((cy - r.top) / Math.max(1, r.height)) * 2 + 1;
        camera.updateMatrixWorld();
        const rc = new THREE.Raycaster();
        rc.setFromCamera(new THREE.Vector2(nx, ny), camera);
        const plane = new THREE.Plane(new THREE.Vector3(0, 1, 0), 0);
        const hit = new THREE.Vector3();
        if (!rc.ray.intersectPlane(plane, hit)) return { x: orbit.tx, y: orbit.tz };
        return { x: hit.x, y: hit.z };
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

      function clearFar() {
        self.far = [];
        self.farSeed = "";
      }

      function rebuildFar() {
        const Look = window.SDLook;
        if (self.far && self.far.length && self.farSeed === self.dirPath) return;
        self.far = Look ? Look.makeFarField(self.dirPath, 640) : [];
        self.farSeed = self.dirPath;
      }

      function farToScreen(p) {
        return {
          x: p.x - orbit.tx + self.w / 2,
          y: p.y - orbit.tz + self.h / 2,
        };
      }

      function paintFar(now) {
        const w = self.w || 1;
        const hgt = self.h || 1;
        farCtx.setTransform(1, 0, 0, 1, 0, 0);
        farCtx.clearRect(0, 0, farCanvas.width, farCanvas.height);
        const dpr = Math.min(2, window.devicePixelRatio || 1);
        farCtx.setTransform(dpr, 0, 0, dpr, 0, 0);
        const Look = window.SDLook;
        if (!Look || !self.far || !self.far.length) return;
        farCtx.globalCompositeOperation = "lighter";
        Look.drawFarField2d(farCtx, self.far, farToScreen, w, hgt, now, 1);
        farCtx.globalCompositeOperation = "source-over";
      }

      function clearSprites() {
        sprites.forEach((s) => {
          scene.remove(s.obj);
          if (s.obj.material) s.obj.material.dispose();
        });
        sprites.length = 0;
        labels.innerHTML = "";
      }

      function rebuild() {
        clearSprites();
        self.stars.forEach((st) => {
          const obj = makeSprite(THREE, st);
          obj.position.set(st.x, st.z || 0, st.y);
          obj.userData.star = st;
          scene.add(obj);
          const lab = document.createElement("div");
          lab.className = isCnsWord(st) ? "c3d-lab cns-word" : "c3d-lab";
          lab.textContent = st.name;
          lab.title = st.name;
          labels.appendChild(lab);
          sprites.push({ obj, lab, star: st, base: obj.userData.baseScale });
        });
      }

      function emitPos() {
        if (h.onPositions) h.onPositions(self.dirPath, cloneMap(self.frozen));
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
        const Look = window.SDLook;
        if (opts.resetCamera) {
          orbit.theta = 0.55;
          orbit.phi = 1.15;
          orbit.r = 420;
          orbit.tx = 0;
          orbit.ty = 0;
          orbit.tz = 0;
        } else if (opts.camera) Object.assign(orbit, opts.camera);
        fit();
        const children = (opts.children || []).map((c) => Object.assign({}, c));
        const saved = cloneMap(opts.savedPositions || {});
        const missing = children.filter((c) => !saved[c.name]);
        let map = saved;
        let changed = false;
        const origin = { x: orbit.tx, y: orbit.tz };
        const dist = Math.max(80, orbit.r || 420);
        const vFov = ((camera && camera.fov ? camera.fov : 50) * Math.PI) / 180;
        const worldH = 2 * dist * Math.tan(vFov / 2);
        const worldW = worldH * (Math.max(1, self.w || 480) / Math.max(1, self.h || 320));
        if (opts.hostLayout) {
          map = {};
          children.forEach((c) => {
            map[c.name] = { x: c.x || 0, y: c.y || 0, z: c.z || 0 };
          });
          changed = false;
        } else if (!Object.keys(saved).length && children.length) {
          map =
            self.layoutMode === "grid"
              ? packGrid(children, origin, worldW, worldH)
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
            if (p) occ[c.name] = { x: p.x, y: p.y, z: p.z || 0, kind: c.kind };
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
            map[name] = Object.assign({}, map[name] || {}, {
              x: pend[name].x,
              y: pend[name].y,
              z: pend[name].z != null ? pend[name].z : (map[name] && map[name].z) || 0,
            });
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
        self.stars = children.map((c) =>
          Object.assign({}, c, {
            x: map[c.name] ? map[c.name].x : 0,
            y: map[c.name] ? map[c.name].y : 0,
            z: map[c.name] ? map[c.name].z || 0 : 0,
            twinkle0: Look ? (Look.hashName(c.name) % 1000) / 1000 : Math.random(),
            dirStyle: c.kind === "dir" && Look ? Look.dirStyle(c.name) : "nebula",
          })
        );
        if (opts.looks) self.looks = opts.looks;
        self.stars.forEach((s) => {
          s.glyph = glyphOf(s, self.looks);
          if (s.glyph && s.glyph.dirStyle) s.dirStyle = s.glyph.dirStyle;
        });
        const keep = new Set(self.stars.map((s) => s.name));
        [...self.sel].forEach((n) => {
          if (!keep.has(n)) self.sel.delete(n);
        });
        rebuild();
        rebuildFar();
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
        const cellW = 56;
        const cellH = 48;
        const viewW = Math.max(1, self.w || 480);
        const dist = Math.max(80, orbit.r || 420);
        const vFov = ((camera && camera.fov ? camera.fov : 50) * Math.PI) / 180;
        const worldH = 2 * dist * Math.tan(vFov / 2);
        const worldW = worldH * (viewW / Math.max(1, self.h || 320));
        const fitCols = Math.max(1, Math.floor((worldW * 0.84) / cellW));
        const cols = Math.max(1, Math.min(n, fitCols));
        const rows = Math.ceil(n / cols);
        const x0 = (orbit.tx || 0) - ((cols - 1) * cellW) / 2;
        const y0 = (orbit.tz || 0) - ((rows - 1) * cellH) / 2;
        items.forEach((s, i) => {
          s.x = x0 + (i % cols) * cellW;
          s.y = y0 + Math.floor(i / cols) * cellH;
          s.z = s.kind === "dir" ? 12 : 0;
        });
        self.arranged = true;
        rebuild();
      }

      function galaxy() {
        self.stars.forEach((s) => {
          const p = self.frozen[s.name];
          if (p) {
            s.x = p.x;
            s.y = p.y;
            s.z = p.z || 0;
          }
        });
        self.arranged = false;
        rebuild();
      }

      function hit(cx, cy) {
        applyCam();
        camera.updateMatrixWorld();
        const r = renderer.domElement.getBoundingClientRect();
        const x = cx - r.left;
        const y = cy - r.top;
        let best = null;
        let bestD = 1e9;
        const vFov = (camera.fov * Math.PI) / 180;
        for (const sp of sprites) {
          const p = project(sp.obj.position);
          if (!p.vis) continue;
          const d = Math.hypot(p.x - x, p.y - y);
          const dist = Math.max(1, camera.position.distanceTo(sp.obj.position));
          const worldPerPx = (2 * dist * Math.tan(vFov / 2)) / Math.max(1, self.h);
          const half = (sp.obj.scale.x || sp.base || 22) * 0.5;
          const lim = half / Math.max(0.001, worldPerPx) + (sp.star.kind === "dir" ? 18 : 10);
          if (d < lim && d < bestD) {
            best = sp.star;
            bestD = d;
          }
        }
        return best;
      }

      function announceSel() {
        if (h.onSelect)
          h.onSelect([...self.sel].map((n) => self.stars.find((s) => s.name === n)).filter(Boolean));
      }

      function applyModClick(s, e) {
        if (!s) return;
        if (e.shiftKey && self.anchor) {
          const pa = sprites.find((x) => x.star === self.anchor);
          const pb = sprites.find((x) => x.star === s);
          if (pa && pb) {
            const sa = project(pa.obj.position);
            const sb = project(pb.obj.position);
            const minx = Math.min(sa.x, sb.x);
            const maxx = Math.max(sa.x, sb.x);
            const miny = Math.min(sa.y, sb.y);
            const maxy = Math.max(sa.y, sb.y);
            sprites.forEach((sp) => {
              const p = project(sp.obj.position);
              if (p.x >= minx && p.x <= maxx && p.y >= miny && p.y <= maxy) {
                self.sel.add(sp.star.name);
              }
            });
          }
        } else if (self.sel.has(s.name)) {
          self.sel.delete(s.name);
        } else {
          self.sel.add(s.name);
        }
        self.anchor = s;
        announceSel();
      }

      let flyGen = 0;
      function cancelFly() {
        flyGen += 1;
        self.flying = false;
        self.flyDir = null;
      }
      function flyTo(end, then) {
        flyGen += 1;
        const gen = flyGen;
        self.flying = true;
        const start = Object.assign({}, orbit);
        const t0 = performance.now();
        function step(now) {
          if (gen !== flyGen) return;
          const t = Math.min(1, (now - t0) / 340);
          const e = t * t * (3 - 2 * t);
          orbit.tx = start.tx + (end.tx - start.tx) * e;
          orbit.ty = start.ty + (end.ty - start.ty) * e;
          orbit.tz = start.tz + (end.tz - start.tz) * e;
          orbit.r = start.r + (end.r - start.r) * e;
          applyCam();
          if (t < 1) requestAnimationFrame(step);
          else {
            self.flying = false;
            self.flyDir = null;
            if (h.onCamera) h.onCamera(orbit);
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
        flyTo(
          {
            tx: star.x,
            ty: star.z || 0,
            tz: star.y,
            r: Math.max(80, orbit.r * 0.35),
          },
          then
        );
      }

      function flyUp(then) {
        flyTo({ tx: orbit.tx, ty: orbit.ty, tz: orbit.tz, r: Math.min(1400, orbit.r * 1.45) }, then);
      }

      function orbitMin() {
        return self.hostLayout ? 15 : 60;
      }
      const ORBIT_MAX = 1800;
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
          .map((st) => ({ star: st, x: st.x, y: st.y, z: st.z || 0 }));
      }
      function zoomCamera(e) {
        const zoomIn = e.deltaY < 0;
        orbit.r = Math.max(orbitMin(), Math.min(ORBIT_MAX, orbit.r * (zoomIn ? 0.9 : 1.1)));
        if (h.onCamera) h.onCamera(orbit);
      }
      function zoomOutMaxed() {
        return orbit.r >= ORBIT_MAX * 0.999;
      }
      function sceneFits() {
        if (!sprites.length) return true;
        applyCam();
        camera.updateMatrixWorld();
        const w = self.w;
        const hgt = self.h;
        const pad = Math.max(24, 0.12 * Math.min(w, hgt));
        for (let i = 0; i < sprites.length; i++) {
          const p = project(sprites[i].obj.position);
          if (!p.vis) return false;
          if (p.x < pad || p.x > w - pad || p.y < pad || p.y > hgt - pad) return false;
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
      container.addEventListener(
        "wheel",
        (e) => {
          if (!self.live || self.dead) return;
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

      function pan3d(dx, dy) {
        const k = orbit.r * 0.0022;
        orbit.tx -= dx * k;
        orbit.ty += dy * k;
        applyCam();
        if (h.onCamera) h.onCamera(orbit);
      }
      function orbit3d(dx, dy) {
        orbit.theta -= dx * 0.008;
        orbit.phi = Math.max(0.12, Math.min(Math.PI - 0.12, orbit.phi - dy * 0.008));
        applyCam();
      }

      function moveGrabbedStar(star, grab, dx, dy, e) {
        const d = self.drag;
        if (!d || !star || !grab) return;
        applyCam();
        camera.updateMatrixWorld();
        const vFov = (camera.fov * Math.PI) / 180;
        const depth = Math.max(1, d.grabDepth || 1);
        const viewH = Math.max(1, d.grabViewH || self.h);
        // Vertical FOV: 1 CSS pixel is the same world size on X and Y.
        // Do not multiply X by aspect — that makes the star run ahead of the mouse.
        const worldPerPx = (2 * depth * Math.tan(vFov / 2)) / viewH;
        const right = d.grabRight || new THREE.Vector3(1, 0, 0);
        const up = d.grabUp || new THREE.Vector3(0, 1, 0);
        let x = grab.x;
        let y = grab.y;
        let z = grab.z;
        if (e && e.shiftKey) {
          z = grab.z + -dy * worldPerPx;
        } else {
          x = grab.x + right.x * dx * worldPerPx + up.x * (-dy) * worldPerPx;
          z = grab.z + right.y * dx * worldPerPx + up.y * (-dy) * worldPerPx;
          y = grab.y + right.z * dx * worldPerPx + up.z * (-dy) * worldPerPx;
        }
        if (!Number.isFinite(x) || !Number.isFinite(y) || !Number.isFinite(z)) return;
        star.x = x;
        star.y = y;
        star.z = z;
        const sp = sprites.find((sp) => sp.star === star);
        if (sp) {
          sp.obj.visible = true;
          sp.obj.position.set(star.x, star.z || 0, star.y);
        }
        self.frozen[star.name] = { x: star.x, y: star.y, z: star.z || 0 };
      }

      function moveGroupFromGrab(dx, dy, e) {
        const d = self.drag;
        if (!d) return;
        const members =
          d.group && d.group.length
            ? d.group
            : [{ star: d.star, x: d.grabX, y: d.grabY, z: d.grabZ }];
        members.forEach((g) => moveGrabbedStar(g.star, g, dx, dy, e));
      }

      container.addEventListener("pointerdown", (e) => {
        if (!self.live || self.dead) return;
        if (e.button !== 0 && e.button !== 1) return;
        e.preventDefault();
        if (h.onActivate) h.onActivate();
        const s = e.button === 0 ? hit(e.clientX, e.clientY) : null;
        const second =
          e.button === 0 &&
          self.lastClick &&
          performance.now() - self.lastClick.t < 500 &&
          Math.hypot(e.clientX - self.lastClick.x, e.clientY - self.lastClick.y) < 12;
        let kind = "select";
        if (e.button === 1) kind = "orbit";
        else if (e.button === 0 && e.altKey && !isNav(e)) kind = "pan";
        else if (e.button === 0 && second && !isNav(e) && !s) kind = "dblpan";
        else if (e.button === 0 && isNav(e)) kind = "nav";
        else if (e.button === 0 && s) kind = "star";
        if (kind !== "star") {
          try {
            container.setPointerCapture(e.pointerId);
          } catch (_) {}
        }
        if (kind === "pan" || kind === "orbit" || kind === "dblpan") {
          self.leaveAcc = 0;
          self.enterAcc = 0;
          self.enterTarget = null;
        }
        const r = container.getBoundingClientRect();
        applyCam();
        camera.updateMatrixWorld();
        let grabDepth = 1;
        let grabRight = null;
        let grabUp = null;
        let grabViewH = Math.max(1, self.h);
        if (s) {
          const pos = new THREE.Vector3(s.x, s.z || 0, s.y);
          const view = pos.clone().applyMatrix4(camera.matrixWorldInverse);
          grabDepth = Math.max(1, -view.z);
          grabRight = new THREE.Vector3();
          grabUp = new THREE.Vector3();
          const back = new THREE.Vector3();
          camera.matrixWorld.extractBasis(grabRight, grabUp, back);
          const canvas = renderer.domElement.getBoundingClientRect();
          grabViewH = Math.max(1, canvas.height);
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
          grabZ: s ? s.z || 0 : 0,
          grabDepth,
          grabRight,
          grabUp,
          grabViewH,
          group: s ? grabGroup(s) : [],
          lastClient: { x: e.clientX, y: e.clientY },
          ghost: null,
          moved: false,
          sx: e.clientX - r.left,
          sy: e.clientY - r.top,
        };
      });

      container.addEventListener("pointermove", (e) => {
        if (!self.live || self.dead) return;
        const r = container.getBoundingClientRect();
        self.mouse.x = e.clientX - r.left;
        self.mouse.y = e.clientY - r.top;
        self.hover = hit(e.clientX, e.clientY);
        if (h.onHover) h.onHover(self.hover);
        const wpos = pointerWorld(e.clientX, e.clientY);
        if (h.onWorldPointer) h.onWorldPointer(wpos, { x: e.clientX, y: e.clientY });
        if (!self.drag) return;
        if (self.drag.kind === "star" && e.clientX === 0 && e.clientY === 0) {
          return;
        }
        const dx = e.clientX - self.drag.lx;
        const dy = e.clientY - self.drag.ly;
        if (Math.hypot(dx, dy) > 3) {
          if (self.drag.kind === "dblpan") self.suppressDblClick = true;
          self.drag.moved = true;
          if (self.drag.kind === "dblpan") self.drag.kind = "pan";
          else if (self.drag.kind === "select") self.drag.kind = "marquee";
        }
        if (self.drag.kind === "orbit") orbit3d(dx, dy);
        else if (self.drag.kind === "pan") pan3d(dx, dy);
        else if (self.drag.kind === "star") {
          moveStarFromGrab(e);
        } else if (self.drag.kind === "marquee") {
          setMarquee(self.drag.sx, self.drag.sy, self.mouse.x, self.mouse.y);
        }
        if (self.drag.kind !== "star") {
          self.drag.lx = e.clientX;
          self.drag.ly = e.clientY;
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

      function setStarSpriteVisible(star, on) {
        const sp = sprites.find((x) => x.star === star);
        if (sp && sp.obj) sp.obj.visible = on;
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
          self.frozen[st.name] = { x: st.x, y: st.y, z: st.z || 0 };
          const sp = sprites.find((x) => x.star === st);
          if (sp) sp.obj.position.set(st.x, st.z || 0, st.y);
        });
      }

      function moveStarFromGrab(e) {
        const d = self.drag;
        if (!d || d.kind !== "star" || !d.star) return;
        if (e.clientX === 0 && e.clientY === 0) return;
        d.lastClient = { x: e.clientX, y: e.clientY };
        if (Math.hypot(e.clientX - d.px, e.clientY - d.py) > 8) d.moved = true;
        if (!d.moved) return;
        const sp = sprites.find((x) => x.star === d.star);
        if (sp) sp.obj.visible = true;
        if (pointerInContainer(e.clientX, e.clientY)) {
          moveGroupFromGrab(e.clientX - d.px, e.clientY - d.py, e);
          hideStarGhost();
        } else if (window.SDStarDrag) {
          if (!d.ghost) d.ghost = window.SDStarDrag.capture(d.star, d.star.kind === "dir" ? 16 : 12);
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
        (d.group || []).forEach((g) => {
          if (g && g.star) setStarSpriteVisible(g.star, true);
        });
        if (d.star) setStarSpriteVisible(d.star, true);
        try {
          container.releasePointerCapture(ev.pointerId);
        } catch (_) {}
        if (
          ev.button === 0 &&
          !d.moved &&
          d.kind !== "dblpan" &&
          d.kind !== "nav" &&
          d.kind !== "pan" &&
          d.kind !== "orbit"
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
          }
          return;
        }
        if (d.kind === "pan" || d.kind === "orbit") return;
        if (d.kind === "marquee") {
          const r = container.getBoundingClientRect();
          const x1 = ev.clientX - r.left;
          const y1 = ev.clientY - r.top;
          const minx = Math.min(d.sx, x1);
          const maxx = Math.max(d.sx, x1);
          const miny = Math.min(d.sy, y1);
          const maxy = Math.max(d.sy, y1);
          const hits = sprites
            .filter((sp) => {
              const p = project(sp.obj.position);
              return p.vis && p.x >= minx && p.x <= maxx && p.y >= miny && p.y <= maxy;
            })
            .map((sp) => sp.star);
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
          const pa = sprites.find((x) => x.star === self.anchor);
          const pb = sprites.find((x) => x.star === s);
          if (pa && pb) {
            const sa = project(pa.obj.position);
            const sb = project(pb.obj.position);
            const minx = Math.min(sa.x, sb.x);
            const maxx = Math.max(sa.x, sb.x);
            const miny = Math.min(sa.y, sb.y);
            const maxy = Math.max(sa.y, sb.y);
            self.sel = new Set(
              sprites
                .filter((sp) => {
                  const p = project(sp.obj.position);
                  return p.x >= minx && p.x <= maxx && p.y >= miny && p.y <= maxy;
                })
                .map((sp) => sp.star.name)
            );
          }
        } else {
          self.sel = new Set([s.name]);
          self.anchor = s;
        }
        announceSel();
      }

      function onWinStarPointer(e) {
        if (!self.live || self.dead) return;
        if (e.clientX === 0 && e.clientY === 0) return;
        if (self.drag && self.drag.kind === "star") moveStarFromGrab(e);
      }

      container.addEventListener("pointerup", endDrag);
      container.addEventListener("pointercancel", (e) => {
        if (!self.live) return;
        if (self.drag && self.drag.kind === "star") endDrag(e);
        else {
          hideMarquee();
          hideStarGhost();
          self.drag = null;
        }
      });
      window.addEventListener("pointermove", onWinStarPointer);
      window.addEventListener("pointerup", (e) => {
        if (self.live && self.drag) endDrag(e);
      });
      window.addEventListener("pointercancel", (e) => {
        if (self.live && self.drag && self.drag.kind === "star") endDrag(e);
      });
      container.addEventListener("dblclick", (e) => {
        if (!self.live || self.dead) return;
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
      container.addEventListener("contextmenu", (e) => {
        if (!self.live || self.dead) return;
        e.preventDefault();
        e.stopPropagation();
        if (h.onContextMenu) h.onContextMenu(e, hit(e.clientX, e.clientY));
      });
      container.addEventListener("dragover", (e) => {
        if (!self.live) return;
        e.preventDefault();
        if (e.dataTransfer) e.dataTransfer.dropEffect = e.shiftKey ? "move" : "copy";
      });
      container.addEventListener("drop", (e) => {
        if (!self.live) return;
        e.preventDefault();
        if (h.onDrop) h.onDrop(e);
      });

      window.addEventListener("keydown", (e) => {
        if (e.key === "Control") self.ctrl = true;
      });
      window.addEventListener("keyup", (e) => {
        if (e.key === "Control") self.ctrl = false;
      });

      function paintLabels() {
        let cnsVis = 0;
        if (self.hostLayout) {
          for (let i = 0; i < sprites.length; i++) {
            const sp = sprites[i];
            if (!isCnsWord(sp.star)) continue;
            const p = project(sp.obj.position);
            if (!p.vis) continue;
            cnsVis++;
            if (cnsVis > 35) break;
          }
        }
        const cnsLabelsOpen =
          !self.hostLayout || cnsVis <= 35 || (90 / Math.max(1, orbit.r)) * 5 >= 36;
        sprites.forEach((sp) => {
          const p = project(sp.obj.position);
          sp.lab.style.left = p.x + "px";
          sp.lab.style.top = p.y + "px";
          const on =
            self.highlight === sp.star.name ||
            self.sel.has(sp.star.name) ||
            self.searchSel.has(sp.star.name) ||
            self.hover === sp.star;
          sp.lab.classList.toggle("sel", self.sel.has(sp.star.name) || self.searchSel.has(sp.star.name));
          sp.lab.classList.toggle("hover", self.hover === sp.star);
          if (isCnsWord(sp.star) && !cnsLabelsOpen && !on) {
            sp.lab.style.opacity = "0";
            return;
          }
          let op = p.vis ? 0.82 : 0;
          if (on) op = p.vis ? 1 : 0;
          if (self.ctrl && p.vis) {
            const d = Math.hypot(p.x - self.mouse.x, p.y - self.mouse.y);
            if (d < 150) op = Math.min(1, op + (1 - d / 150) * 0.7);
          }
          sp.lab.style.opacity = String(op);
          sp.lab.style.color =
            self.sel.has(sp.star.name) || self.searchSel.has(sp.star.name)
              ? "rgba(255, 232, 208, 0.95)"
              : "";
        });
      }

      function loop() {
        if (self.dead || !self.live) return;
        applyCam();
        const now = performance.now();
        const Look = window.SDLook;
        sprites.forEach((sp) => {
          const st = sp.star;
          const selected = self.sel.has(st.name) || self.searchSel.has(st.name);
          const on = self.hover === st || self.highlight === st.name || selected;
          const phase = st.twinkle0 || 0;
          let k = 1;
          if (Look) {
            k =
              st.kind === "dir"
                ? 0.92 + 0.12 * Look.twinkleAt(phase, now, false)
                : 0.78 + 0.32 * Look.twinkleAt(phase, now, true);
          }
          if (on) k *= 1.22;
          if (selected) k *= 1.35;
          const b = sp.base || 36;
          sp.obj.scale.set(b * k, b * k, 1);
          sp.obj.material.opacity = 1;
          if (sp.obj.material.color) sp.obj.material.color.setHex(selected ? 0xfff1c8 : 0xffffff);
        });
        paintFar(now);
        renderer.render(scene, camera);
        paintLabels();
        kickLoop();
      }

      let loopRaf = 0;
      function kickLoop() {
        if (self.dead) return;
        if (loopRaf) {
          cancelAnimationFrame(loopRaf);
          loopRaf = 0;
        }
        loopRaf = requestAnimationFrame(() => {
          loopRaf = 0;
          loop();
        });
      }

      const ro = new ResizeObserver(() => fit());
      ro.observe(container);
      fit();
      applyCam();
      kickLoop();

      return {
        setScene,
        arrangeBy,
        galaxy,
        flyEnter,
        flyUp,
        flyTo(x, y, scale, worldR) {
          flyTo({
            tx: x,
            ty: 0,
            tz: y,
            r:
              worldR != null
                ? Math.max(70, worldR)
                : scale != null
                  ? Math.max(70, 160 / Math.max(0.2, scale))
                  : Math.max(90, orbit.r * 0.4),
          });
        },
        worldBBox() {
          const r = Math.max(120, orbit.r);
          return {
            xmin: orbit.tx - r,
            xmax: orbit.tx + r,
            ymin: orbit.tz - r,
            ymax: orbit.tz + r,
            scale: Math.max(0.12, 90 / r),
          };
        },
        getCamera() {
          return Object.assign({}, orbit);
        },
        setCamera(c) {
          Object.assign(orbit, c || {});
        },
        getPositions() {
          return cloneMap(self.frozen);
        },
        highlight(name) {
          self.highlight = name;
        },
        highlightParent(parentPath) {
          const s = self.stars.find((x) => x.path === parentPath);
          self.highlight = s ? s.name : null;
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
        worldFromClient(cx, cy) {
          if (cx == null || cy == null) return { x: orbit.tx, y: orbit.tz };
          return pointerWorld(cx, cy);
        },
        screenOf(star) {
          const sp = sprites.find((x) => x.star.name === star.name);
          if (!sp) return { x: 0, y: 0, r: 12 };
          const r = renderer.domElement.getBoundingClientRect();
          const p = project(sp.obj.position);
          return { x: r.left + p.x, y: r.top + p.y, r: sp.star.kind === "dir" ? 16 : 12 };
        },
        notePending(name, x, y, kind) {
          if (!self.pending) self.pending = {};
          const blockers = hintBlockers(self.stars, self.pending, name);
          const p =
            self.layoutMode === "grid"
              ? emptyGridCell(x, y, blockers)
              : spiralAway(x, y, blockers, kind);
          self.pending[name] = { x: p.x, y: p.y, z: p.z || (kind === "dir" ? 10 : 0), kind: kind || "file" };
        },
        placeHint() {
          return { x: orbit.tx, y: orbit.tz };
        },
        resize: fit,
        setLooks(map) {
          self.looks = map || {};
          self.stars.forEach((s) => {
            s.glyph = glyphOf(s, self.looks);
            if (s.glyph && s.glyph.dirStyle) s.dirStyle = s.glyph.dirStyle;
          });
          rebuild();
        },
        setLive(on) {
          self.live = !!on;
          if (self.live) kickLoop();
          if (!self.live) {
            self.drag = null;
            hideMarquee();
            hideStarGhost();
          }
        },
        domElement: renderer.domElement,
        captureDataUrl(opts) {
          applyCam();
          paintFar(performance.now());
          renderer.render(scene, camera);
          const type = (opts && opts.type) || "image/jpeg";
          const quality = opts && opts.quality != null ? opts.quality : 0.72;
          const maxEdge = (opts && opts.maxEdge) || 1280;
          const src = renderer.domElement;
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
            const tmp = document.createElement("canvas");
            tmp.width = dw;
            tmp.height = dh;
            const g = tmp.getContext("2d");
            if (!g) return src.toDataURL(type, quality);
            g.drawImage(farCanvas, 0, 0, dw, dh);
            g.drawImage(src, 0, 0, dw, dh);
            return tmp.toDataURL(type, quality);
          } catch (_) {
            return "";
          }
        },
        statusEl: status,
        destroy() {
          self.dead = true;
          ro.disconnect();
          clearSprites();
          clearFar();
          renderer.dispose();
          host.remove();
          labels.remove();
          status.remove();
        },
      };
    },
  };
})();
