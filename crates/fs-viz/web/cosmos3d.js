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
    return kind === "dir" ? 40 : 20;
  }

  function placeUnoccupied(missing, saved) {
    const occ = [];
    Object.keys(saved || {}).forEach((k) => {
      const p = saved[k];
      if (!p) return;
      occ.push({ x: p.x, y: p.y, z: p.z || 0, sep: starSep(p.kind) });
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
        y: Math.sin(a) * r * 0.85,
        z: (s.kind === "dir" ? 12 : 0) + 8 * Math.sin((i + 1) * GOLDEN),
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
      pos[b.name] = { x: b.x, y: b.y, z: b.z };
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
    const z0 = kind === "dir" ? 10 : 0;
    const blocked = (px, py) => {
      for (let i = 0; i < blockers.length; i++) {
        const o = blockers[i];
        const need = selfSep + starSep(o.kind);
        if (Math.hypot(px - o.x, py - o.y) < need) return true;
      }
      return false;
    };
    if (!blocked(x, y)) return { x, y, z: z0 };
    for (let i = 1; i < 96; i++) {
      const a = i * GOLDEN;
      const r = selfSep * 2 + 12 * Math.sqrt(i);
      const px = x + Math.cos(a) * r;
      const py = y + Math.sin(a) * r;
      const pz = z0 + 6 * Math.sin(i * GOLDEN);
      if (!blocked(px, py)) return { x: px, y: py, z: pz };
    }
    return { x: x + selfSep * 2, y, z: z0 + 4 };
  }

  function cloneMap(m) {
    const o = {};
    Object.keys(m || {}).forEach((k) => {
      const p = m[k];
      if (p) o[k] = { x: p.x, y: p.y, z: p.z || 0 };
    });
    return o;
  }

  function dummy(container) {
    const msg = document.createElement("div");
    msg.className = "c3d-missing";
    msg.textContent =
      "Three.js failed to load from the CDN. Toggle back to 2D, or check the network and retry.";
    container.appendChild(msg);
    return {
      missing: true,
      setScene() {},
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
      notePending() {},
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
        sel: new Set(),
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
        self.dirPath = opts.dirPath || "";
        const Look = window.SDLook;
        const children = (opts.children || []).map((c) => Object.assign({}, c));
        const saved = cloneMap(opts.savedPositions || {});
        const missing = children.filter((c) => !saved[c.name]);
        let map = saved;
        let changed = false;
        if (opts.hostLayout) {
          map = {};
          children.forEach((c) => {
            map[c.name] = { x: c.x || 0, y: c.y || 0, z: c.z || 0 };
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
            if (p) occ[c.name] = { x: p.x, y: p.y, z: p.z || 0, kind: c.kind };
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
            map[name] = Object.assign({}, map[name] || {}, {
              x: pend[name].x,
              y: pend[name].y,
              z: pend[name].z != null ? pend[name].z : (map[name] && map[name].z) || 0,
            });
            changed = true;
          });
        }
        self.pending = {};
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
          s.glyph = (self.looks && (self.looks[lookKey(s.path)] || self.looks[s.path])) || null;
          if (s.glyph && s.glyph.dirStyle) s.dirStyle = s.glyph.dirStyle;
        });
        const keep = new Set(self.stars.map((s) => s.name));
        [...self.sel].forEach((n) => {
          if (!keep.has(n)) self.sel.delete(n);
        });
        rebuild();
        rebuildFar();
        if (changed && !opts.ephemeral) emitPos();
        if (opts.resetCamera) {
          orbit.theta = 0.55;
          orbit.phi = 1.15;
          orbit.r = 420;
          orbit.tx = 0;
          orbit.ty = 0;
          orbit.tz = 0;
        } else if (opts.camera) Object.assign(orbit, opts.camera);
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
          const r = 48 + n * 8;
          items.forEach((s, i) => {
            const a = (i / n) * Math.PI * 2 - Math.PI / 2;
            s.x = Math.cos(a) * r;
            s.y = Math.sin(a) * r;
            s.z = s.kind === "dir" ? 12 : 0;
          });
        } else {
          items.forEach((s, i) => {
            s.x = (i - (n - 1) / 2) * 46;
            s.y = i % 2 === 0 ? -16 : 16;
            s.z = s.kind === "dir" ? 12 : 0;
          });
        }
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

      function moveGrabbedStar(star, dx, dy, e) {
        applyCam();
        camera.updateMatrixWorld();
        const pos = new THREE.Vector3(star.x, star.z || 0, star.y);
        const dist = Math.max(1, camera.position.distanceTo(pos));
        const vFov = (camera.fov * Math.PI) / 180;
        const worldPerPxY = (2 * dist * Math.tan(vFov / 2)) / Math.max(1, self.h);
        const worldPerPxX = worldPerPxY * camera.aspect;
        const right = new THREE.Vector3();
        const up = new THREE.Vector3();
        const back = new THREE.Vector3();
        camera.matrixWorld.extractBasis(right, up, back);
        if (e && e.shiftKey) {
          star.z = (star.z || 0) + -dy * worldPerPxY;
        } else {
          const d = new THREE.Vector3()
            .addScaledVector(right, dx * worldPerPxX)
            .addScaledVector(up, -dy * worldPerPxY);
          star.x += d.x;
          star.z = (star.z || 0) + d.y;
          star.y += d.z;
        }
        const sp = sprites.find((x) => x.star === star);
        if (sp) sp.obj.position.set(star.x, star.z || 0, star.y);
        if (!self.arranged) {
          self.frozen[star.name] = { x: star.x, y: star.y, z: star.z || 0 };
        }
      }

      container.addEventListener("pointerdown", (e) => {
        if (!self.live || self.dead) return;
        if (e.button !== 0 && e.button !== 1) return;
        e.preventDefault();
        if (h.onActivate) h.onActivate();
        try {
          container.setPointerCapture(e.pointerId);
        } catch (_) {}
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
        if (kind === "pan" || kind === "orbit" || kind === "dblpan") {
          self.leaveAcc = 0;
          self.enterAcc = 0;
          self.enterTarget = null;
        }
        const r = container.getBoundingClientRect();
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

      container.addEventListener("pointermove", (e) => {
        if (!self.live || self.dead) return;
        const r = container.getBoundingClientRect();
        self.mouse.x = e.clientX - r.left;
        self.mouse.y = e.clientY - r.top;
        self.hover = hit(e.clientX, e.clientY);
        if (h.onHover) h.onHover(self.hover);
        const wpos = self.hover ? { x: self.hover.x, y: self.hover.y } : { x: orbit.tx, y: orbit.tz };
        if (h.onWorldPointer) h.onWorldPointer(wpos, { x: e.clientX, y: e.clientY });
        if (!self.drag) return;
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
        else if (self.drag.kind === "star" && self.drag.star) {
          moveGrabbedStar(self.drag.star, dx, dy, e);
        } else if (self.drag.kind === "marquee") {
          setMarquee(self.drag.sx, self.drag.sy, self.mouse.x, self.mouse.y);
        }
        self.drag.lx = e.clientX;
        self.drag.ly = e.clientY;
      });

      function endDrag(e) {
        if (!self.live) return;
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
          d.kind !== "pan" &&
          d.kind !== "orbit"
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
        if (d.kind === "pan" || d.kind === "orbit") return;
        if (d.kind === "marquee") {
          const r = container.getBoundingClientRect();
          const x1 = e.clientX - r.left;
          const y1 = e.clientY - r.top;
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
          const t = d.star || hit(e.clientX, e.clientY);
          if (t && t.kind === "dir") enterStar(t);
          else if (!t) leaveScene();
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

      container.addEventListener("pointerup", endDrag);
      container.addEventListener("pointercancel", () => {
        if (!self.live) return;
        hideMarquee();
        self.drag = null;
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
            self.highlight === sp.star.name || self.sel.has(sp.star.name) || self.hover === sp.star;
          sp.lab.classList.toggle("sel", self.sel.has(sp.star.name));
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
          sp.lab.style.color = self.sel.has(sp.star.name) ? "rgba(255, 232, 208, 0.95)" : "";
        });
      }

      function loop() {
        if (self.dead) return;
        applyCam();
        const now = performance.now();
        const Look = window.SDLook;
        sprites.forEach((sp) => {
          const st = sp.star;
          const selected = self.sel.has(st.name);
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
        worldFromClient() {
          return { x: orbit.tx, y: orbit.tz };
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
          const p = spiralAway(x, y, hintBlockers(self.stars, self.pending, name), kind);
          self.pending[name] = { x: p.x, y: p.y, z: p.z || 0, kind: kind || "file" };
        },
        placeHint() {
          return { x: orbit.tx, y: orbit.tz };
        },
        resize: fit,
        setLooks(map) {
          self.looks = map || {};
          self.stars.forEach((s) => {
            s.glyph = (self.looks && (self.looks[lookKey(s.path)] || self.looks[s.path])) || null;
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
