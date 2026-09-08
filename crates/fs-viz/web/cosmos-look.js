(function () {
  function hashName(s) {
    let h = 2166136261;
    const t = String(s || "");
    for (let i = 0; i < t.length; i++) h = Math.imul(h ^ t.charCodeAt(i), 16777619);
    return h >>> 0;
  }

  function mulberry(seed) {
    let a = seed >>> 0 || 1;
    return function () {
      a |= 0;
      a = (a + 0x6d2b79f5) | 0;
      let t = Math.imul(a ^ (a >>> 15), 1 | a);
      t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
      return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
    };
  }

  const DIR_KINDS = [
    "emission",
    "reflection",
    "planetary",
    "snr",
    "spiral",
    "barred",
    "elliptical",
    "irregular",
    "cluster",
    "edgeon",
  ];
  const STAR_SPECS = ["o", "b", "a", "f", "g", "k", "m", "wr"];
  const STAR_MORPHS = ["spike4", "halo", "binary", "spike6", "airy"];
  const SPEC_RGB = {
    o: { cr: [155, 185, 255], mid: [70, 120, 255] },
    b: { cr: [180, 210, 255], mid: [110, 160, 255] },
    a: { cr: [236, 244, 255], mid: [160, 198, 255] },
    f: { cr: [255, 248, 225], mid: [255, 220, 160] },
    g: { cr: [255, 228, 165], mid: [255, 190, 90] },
    k: { cr: [255, 186, 110], mid: [255, 140, 60] },
    m: { cr: [255, 130, 90], mid: [255, 80, 50] },
    wr: { cr: [255, 140, 210], mid: [255, 70, 160] },
  };

  function dirStyle(name) {
    return DIR_KINDS[hashName(name) % DIR_KINDS.length];
  }

  function starLook(name) {
    const h = hashName(name);
    return {
      spec: STAR_SPECS[h % STAR_SPECS.length],
      morph: STAR_MORPHS[(h >>> 4) % STAR_MORPHS.length],
    };
  }

  function twinkleAt(phase, now, fast) {
    const t = now / 1000;
    const period = fast ? 0.6 + phase * 0.8 : 2.4 + phase * 1.2;
    return 0.5 + 0.5 * Math.sin((t / period) * Math.PI * 2 + phase * Math.PI * 2);
  }

  function rgba(r, g, b, a) {
    return "rgba(" + r + "," + g + "," + b + "," + a + ")";
  }

  function perceptRgb(r0, g0, b0) {
    const sum = r0 + g0 + b0 + 1;
    const gShare = g0 / sum;
    const mx = Math.max(r0, g0, b0, 1);
    if (gShare > 0.34 && mx < 252) {
      const lift = 1.22 + (1 - r0 / 255) * 0.18;
      r0 = Math.min(255, r0 * 1.16 + 28);
      g0 = Math.min(255, g0 * lift + 22);
      b0 = Math.min(255, b0 * 1.12 + 18);
    }
    return [r0, g0, b0];
  }

  let objectGlow = 1;
  function fillSoft(ctx, x, y, rx, ry, ang, r0, g0, b0, a0, aMid) {
    const rgb = perceptRgb(r0, g0, b0);
    r0 = rgb[0];
    g0 = rgb[1];
    b0 = rgb[2];
    a0 = Math.min(1, a0 * objectGlow);
    if (aMid != null) aMid = Math.min(1, aMid * objectGlow);
    if (rx < 0.4 || ry < 0.4 || a0 <= 0) return;
    ctx.save();
    ctx.translate(x, y);
    ctx.rotate(ang || 0);
    ctx.scale(Math.max(0.4, rx), Math.max(0.4, ry));
    const grd = ctx.createRadialGradient(0, 0, 0, 0, 0, 1);
    grd.addColorStop(0, rgba(r0, g0, b0, a0));
    grd.addColorStop(0.42, rgba(r0, g0, b0, aMid != null ? aMid : a0 * 0.38));
    grd.addColorStop(1, rgba(r0, g0, b0, 0));
    ctx.fillStyle = grd;
    ctx.beginPath();
    ctx.arc(0, 0, 1, 0, Math.PI * 2);
    ctx.fill();
    ctx.restore();
  }

  function diffraction(ctx, len, thick, r, g, b, a) {
    if (len < 1 || a <= 0) return;
    ctx.save();
    ctx.scale(len, Math.max(0.6, thick));
    const grd = ctx.createRadialGradient(0, 0, 0, 0, 0, 1);
    grd.addColorStop(0, rgba(r, g, b, a));
    grd.addColorStop(0.12, rgba(r, g, b, a * 0.45));
    grd.addColorStop(0.45, rgba(r, g, b, a * 0.08));
    grd.addColorStop(1, rgba(r, g, b, 0));
    ctx.fillStyle = grd;
    ctx.beginPath();
    ctx.arc(0, 0, 1, 0, Math.PI * 2);
    ctx.fill();
    ctx.restore();
  }

  function paintFileStar(ctx, x, y, r, opts) {
    objectGlow = 1.75;
    try {
    const on = !!(opts && opts.on);
    const tw = opts && opts.twinkle != null ? opts.twinkle : 1;
    const spec = (opts && opts.spec) || (opts && opts.warm ? "k" : "a");
    const morph = (opts && opts.morph) || (opts && opts.spikes === false ? "halo" : "spike4");
    const pal = SPEC_RGB[spec] || SPEC_RGB.a;
    const cr = pal.cr;
    const mid = pal.mid;
    const rot = opts && opts.rot != null ? opts.rot : 0;
    const R = r * (0.8 + 0.22 * tw) * (on ? 1.16 : 1);
    const bloom = on ? 1.25 : 1;
    const a = (on ? 0.55 : 0.32) * (0.65 + 0.35 * tw);
    ctx.save();
    ctx.translate(x, y);
    ctx.rotate(rot);
    fillSoft(ctx, 0, 0, R * 3.8 * bloom, R * 3.8 * bloom, 0, mid[0], mid[1], mid[2], on ? 0.22 : 0.1, on ? 0.08 : 0.035);
    fillSoft(ctx, 0, 0, R * 2.1, R * 2.1, 0, cr[0], cr[1], cr[2], on ? 0.4 : 0.2, 0.08);
    if (morph === "airy") {
      fillSoft(ctx, 0, 0, R * 1.55, R * 1.55, 0, cr[0], cr[1], cr[2], 0.12, 0.02);
      fillSoft(ctx, 0, 0, R * 2.6, R * 2.6, 0, mid[0], mid[1], mid[2], 0.08, 0.015);
    }
    if (morph === "spike4" || morph === "airy") {
      diffraction(ctx, R * (3.4 + 1.1 * tw), R * 0.11, cr[0], cr[1], cr[2], a * (morph === "airy" ? 0.55 : 1));
      ctx.save();
      ctx.rotate(Math.PI / 2);
      diffraction(ctx, R * (3.1 + 1.0 * tw), R * 0.1, cr[0], cr[1], cr[2], a * 0.92 * (morph === "airy" ? 0.55 : 1));
      ctx.restore();
    }
    if (morph === "spike6") {
      for (let i = 0; i < 3; i++) {
        ctx.save();
        ctx.rotate((i * Math.PI) / 3);
        diffraction(ctx, R * (3.0 + tw), R * 0.09, cr[0], cr[1], cr[2], a);
        ctx.restore();
      }
    }
    if (morph === "binary") {
      const ox = R * 0.55;
      fillSoft(ctx, -ox, 0, R * 0.7, R * 0.7, 0, cr[0], cr[1], cr[2], 0.7, 0.2);
      fillSoft(ctx, ox * 0.85, R * 0.12, R * 0.5, R * 0.5, 0, mid[0], mid[1], mid[2], 0.65, 0.18);
      fillSoft(ctx, -ox, 0, R * 0.28, R * 0.28, 0, 255, 255, 255, 1, 0.45);
      fillSoft(ctx, ox * 0.85, R * 0.12, R * 0.2, R * 0.2, 0, 255, 255, 255, 0.95, 0.4);
    } else {
      fillSoft(ctx, 0, 0, R * 0.85, R * 0.85, 0, 255, 255, 255, 0.95, 0.35);
      fillSoft(ctx, 0, 0, R * 0.35, R * 0.35, 0, 255, 255, 255, 1, 0.5);
    }
    ctx.restore();
    } finally {
      objectGlow = 1;
    }
  }

  function paintArms(ctx, R, k, rng, arms, palIn, palOut) {
    const knots = 32;
    for (let arm = 0; arm < arms; arm++) {
      for (let i = 0; i < knots; i++) {
        const u = i / knots;
        const a = arm * ((Math.PI * 2) / arms) + u * 3.5 + (rng() - 0.5) * 0.12;
        const rr = R * (0.14 + u * 0.95);
        const px = Math.cos(a) * rr;
        const py = Math.sin(a) * rr * (0.48 + rng() * 0.12);
        const sz = R * (0.07 + (1 - u) * 0.1) * (0.7 + rng() * 0.5);
        const c0 = u < 0.35 ? palIn : palOut;
        fillSoft(ctx, px, py, sz, sz * 0.7, a, c0[0], c0[1], c0[2], (0.16 + (1 - u) * 0.12) * k, 0.04);
      }
    }
  }

  function paintDirNebula(ctx, x, y, r, opts) {
    objectGlow = 1.75;
    try {
    const on = !!(opts && opts.on);
    const br = opts && opts.breathe != null ? opts.breathe : 1;
    let style = (opts && opts.style) || "emission";
    if (style === "nebulaA" || style === "nebulaB") style = "emission";
    if (style === "spiralA" || style === "spiralB") style = "spiral";
    const rot = (opts && opts.rot) || 0;
    const rng = mulberry((opts && opts.seed) || hashName(style) || 1);
    const R = r * 1.85 * (0.92 + 0.08 * br) * (on ? 1.12 : 1);
    const k = on ? 1.35 : 1;
    ctx.save();
    ctx.translate(x, y);
    ctx.rotate(rot * 0.35);

    if (style === "emission") {
      fillSoft(ctx, 0, 0, R * 1.4, R * 1.05, 0.3, 40, 20, 80, 0.16 * k, 0.05);
      fillSoft(ctx, R * 0.1, 0, R * 0.9, R * 0.65, rng(), 220, 50, 140, 0.32 * k, 0.1);
      fillSoft(ctx, -R * 0.15, R * 0.08, R * 0.7, R * 0.5, rng(), 255, 120, 60, 0.22 * k, 0.07);
      fillSoft(ctx, R * 0.2, -R * 0.1, R * 0.45, R * 0.35, rng(), 80, 160, 255, 0.18 * k, 0.05);
    } else if (style === "reflection") {
      fillSoft(ctx, 0, 0, R * 1.25, R * 0.95, 0.2, 30, 50, 120, 0.2 * k, 0.06);
      fillSoft(ctx, -R * 0.1, 0, R * 0.8, R * 0.6, rng(), 120, 180, 255, 0.28 * k, 0.08);
      fillSoft(ctx, R * 0.15, R * 0.05, R * 0.5, R * 0.4, rng(), 200, 220, 255, 0.18 * k, 0.05);
    } else if (style === "planetary") {
      fillSoft(ctx, 0, 0, R * 1.15, R * 1.05, 0, 50, 220, 190, 0.32 * k, 0.1);
      for (let i = 0; i < 18; i++) {
        const a = (i / 18) * Math.PI * 2;
        fillSoft(
          ctx,
          Math.cos(a) * R * 0.62,
          Math.sin(a) * R * 0.52,
          R * 0.16,
          R * 0.12,
          a,
          90,
          255,
          220,
          0.32 * k,
          0.08
        );
      }
      fillSoft(ctx, 0, 0, R * 0.22, R * 0.22, 0, 255, 255, 220, 0.85 * k, 0.25);
    } else if (style === "snr") {
      fillSoft(ctx, 0, 0, R * 1.3, R * 1.15, rng(), 40, 140, 110, 0.22 * k, 0.07);
      for (let i = 0; i < 10; i++) {
        fillSoft(
          ctx,
          (rng() - 0.5) * R * 1.1,
          (rng() - 0.5) * R * 1.0,
          R * (0.15 + rng() * 0.28),
          R * (0.08 + rng() * 0.16),
          rng() * Math.PI,
          rng() > 0.5 ? 60 : 40,
          rng() > 0.4 ? 220 : 160,
          rng() > 0.5 ? 180 : 255,
          (0.2 + rng() * 0.16) * k,
          0.06
        );
      }
    } else if (style === "spiral" || style === "barred") {
      fillSoft(ctx, 0, 0, R * 1.2, R * 0.5, 0.12, 70, 50, 120, 0.14 * k, 0.04);
      paintArms(ctx, R, k, rng, style === "barred" ? 2 : 2, [255, 190, 120], [90, 150, 255]);
      if (style === "barred") {
        fillSoft(ctx, 0, 0, R * 0.7, R * 0.14, 0.1, 255, 200, 140, 0.28 * k, 0.08);
      }
      fillSoft(ctx, 0, 0, R * 0.28, R * 0.24, 0, 255, 220, 160, 0.7 * k, 0.2);
    } else if (style === "elliptical") {
      fillSoft(ctx, 0, 0, R * 1.15, R * 0.85, 0.2, 255, 200, 130, 0.28 * k, 0.08);
      fillSoft(ctx, 0, 0, R * 0.7, R * 0.55, 0.2, 255, 230, 180, 0.35 * k, 0.12);
      fillSoft(ctx, 0, 0, R * 0.22, R * 0.2, 0, 255, 250, 230, 0.7 * k, 0.25);
    } else if (style === "irregular") {
      fillSoft(ctx, 0, 0, R * 1.05, R * 0.85, rng(), 255, 140, 90, 0.22 * k, 0.07);
      for (let i = 0; i < 9; i++) {
        const pal = i % 3 === 0 ? [255, 110, 160] : i % 3 === 1 ? [120, 200, 255] : [255, 190, 90];
        fillSoft(
          ctx,
          (rng() - 0.5) * R * 1.1,
          (rng() - 0.5) * R * 0.9,
          R * (0.22 + rng() * 0.38),
          R * (0.16 + rng() * 0.24),
          rng() * Math.PI,
          pal[0],
          pal[1],
          pal[2],
          (0.24 + rng() * 0.18) * k,
          0.08
        );
      }
      fillSoft(ctx, 0, 0, R * 0.28, R * 0.24, 0, 255, 236, 220, 0.7 * k, 0.22);
    } else if (style === "cluster") {
      fillSoft(ctx, 0, 0, R * 1.1, R * 1.1, 0, 120, 140, 200, 0.2 * k, 0.06);
      for (let i = 0; i < 22; i++) {
        const a = rng() * Math.PI * 2;
        const rr = Math.pow(rng(), 0.7) * R * 0.85;
        fillSoft(
          ctx,
          Math.cos(a) * rr,
          Math.sin(a) * rr * 0.9,
          R * (0.04 + rng() * 0.05),
          R * (0.04 + rng() * 0.05),
          0,
          255,
          230,
          rng() > 0.5 ? 200 : 255,
          0.45 * k,
          0.1
        );
      }
      fillSoft(ctx, 0, 0, R * 0.2, R * 0.2, 0, 255, 250, 240, 0.8 * k, 0.25);
    } else if (style === "edgeon") {
      fillSoft(ctx, 0, 0, R * 1.45, R * 0.28, 0.05, 210, 190, 255, 0.32 * k, 0.1);
      fillSoft(ctx, 0, 0, R * 1.2, R * 0.1, 0.05, 12, 8, 16, 0.45, 0.12);
      fillSoft(ctx, 0, 0, R * 0.25, R * 0.18, 0, 255, 220, 170, 0.55 * k, 0.15);
    } else {
      fillSoft(ctx, 0, 0, R * 1.2, R * 0.9, rng(), 180, 70, 140, 0.25 * k, 0.08);
    }

    if (style !== "cluster" && style !== "planetary" && style !== "edgeon" && style !== "elliptical") {
      fillSoft(ctx, 0, 0, R * 0.22, R * 0.2, 0, 255, 236, 255, on ? 0.7 : 0.42, 0.16);
    }
    ctx.restore();
    } finally {
      objectGlow = 1;
    }
  }

  const FAR_TILE_X = 2800;
  const FAR_TILE_Y = 2100;
  const FAR_PARALLAX = 0.45;
  const FAR_WRAP_CAP = 12;

  function makeFarField(seed, count) {
    const rng = mulberry(hashName(String(seed)) || 1);
    const n = count || 560;
    const pts = [];
    for (let i = 0; i < 6; i++) {
      pts.push({
        x: (rng() - 0.5) * 2400,
        y: (rng() - 0.5) * 1800,
        z: (rng() - 0.5) * 400,
        mag: 0.35 + rng() * 0.4,
        warm: rng() > 0.5,
        phase: rng(),
        layer: 0,
        kind: "neb",
        style: DIR_KINDS[i % 4],
        rx: 160 + rng() * 260,
        ry: 90 + rng() * 160,
        rot: rng() * Math.PI,
        tint: rng(),
      });
    }
    for (let i = 0; i < n; i++) {
      const roll = rng();
      const kind =
        roll < 0.12 ? "gal" : roll < 0.18 ? "cluster" : roll < 0.22 ? "ring" : roll < 0.32 ? "spike" : "star";
      pts.push({
        x: (rng() - 0.5) * FAR_TILE_X,
        y: (rng() - 0.5) * FAR_TILE_Y,
        z: (rng() - 0.5) * 900,
        mag: Math.pow(rng(), 1.6) * 1.15 + 0.12,
        warm: rng() > 0.82,
        phase: rng(),
        layer: rng() > 0.55 ? 1 : 0,
        kind,
        style: DIR_KINDS[Math.floor(rng() * DIR_KINDS.length)],
        spec: STAR_SPECS[Math.floor(rng() * STAR_SPECS.length)],
        rx: 8 + rng() * 28,
        ry: 4 + rng() * 14,
        rot: rng() * Math.PI,
        tint: rng(),
      });
    }
    return pts;
  }

  function farCamera(toScreen, w, h, camScale) {
    const o = toScreen({ x: 0, y: 0 });
    const s = Math.max(1e-6, camScale || 1);
    return { x: (w * 0.5 - o.x) / s, y: (h * 0.5 - o.y) / s };
  }

  function wrapAxis(pos, period, min, max, cap) {
    const out = [];
    if (!(period > 1e-3)) {
      out.push(pos);
      return out;
    }
    let k0 = Math.ceil((min - pos) / period);
    let k1 = Math.floor((max - pos) / period);
    if (k1 < k0) return out;
    if (k1 - k0 > cap) {
      const mid = (k0 + k1) * 0.5;
      k0 = Math.round(mid - cap * 0.5);
      k1 = k0 + cap;
    }
    for (let k = k0; k <= k1; k++) out.push(pos + k * period);
    return out;
  }

  function drawFarField2d(ctx, pts, toScreen, w, h, now, camScale) {
    const t = now / 1000;
    const zoom = 1;
    const cam = farCamera(toScreen, w, h, camScale);
    function eachCopy(d, pad, fn) {
      const sx = d.x - cam.x * FAR_PARALLAX + w * 0.5;
      const sy = d.y - cam.y * FAR_PARALLAX + h * 0.5;
      const xs = wrapAxis(sx, FAR_TILE_X, -pad, w + pad, FAR_WRAP_CAP);
      const ys = wrapAxis(sy, FAR_TILE_Y, -pad, h + pad, FAR_WRAP_CAP);
      for (let i = 0; i < xs.length; i++) {
        for (let j = 0; j < ys.length; j++) fn({ x: xs[i], y: ys[j] });
      }
    }
    for (let i = 0; i < pts.length; i++) {
      const d = pts[i];
      if (d.kind !== "neb") continue;
      const st = d.style || "emission";
      let r0 = 180, g0 = 50, b0 = 140;
      if (st === "reflection") { r0 = 80; g0 = 140; b0 = 255; }
      else if (st === "snr") { r0 = 50; g0 = 200; b0 = 160; }
      else if (st === "planetary") { r0 = 60; g0 = 220; b0 = 180; }
      else if (d.tint > 0.66) { r0 = 255; g0 = 110; b0 = 70; }
      eachCopy(d, 280, (p) => {
        fillSoft(ctx, p.x, p.y, Math.min(6, d.rx * zoom * 0.04), Math.min(4.5, d.ry * zoom * 0.04), d.rot, r0, g0, b0, 0.14 + d.mag * 0.12, 0.04);
      });
    }
    for (let i = 0; i < pts.length; i++) {
      const d = pts[i];
      if (d.kind === "neb") continue;
      const tw = 0.62 + 0.38 * Math.sin(t * (0.28 + d.phase * 0.32) + d.phase * 6);
      const dim = Math.min(1, (0.18 + d.mag * 0.55) * 1.9 * tw * (d.layer ? 0.65 : 1));
      eachCopy(d, 24, (p) => {
        if (d.kind === "gal" || d.kind === "cluster" || d.kind === "ring") {
          const st = d.style || "spiral";
          let r0 = 140, g0 = 90, b0 = 255;
          if (st === "elliptical") { r0 = 255; g0 = 200; b0 = 130; }
          else if (st === "emission") { r0 = 255; g0 = 90; b0 = 140; }
          else if (st === "snr") { r0 = 70; g0 = 210; b0 = 170; }
          else if (st === "reflection") { r0 = 100; g0 = 170; b0 = 255; }
          else if (st === "irregular") { r0 = 255; g0 = 160; b0 = 80; }
          else if (d.tint < 0.33) { r0 = 255; g0 = 140; b0 = 90; }
          if (d.kind === "cluster") {
            for (let j = 0; j < 7; j++) {
              fillSoft(
                ctx,
                p.x + (j - 3) * 1.4 * zoom,
                p.y + ((j % 3) - 1) * 1.2 * zoom,
                1.1 * zoom,
                1.1 * zoom,
                0,
                255,
                235,
                210,
                dim * 0.55,
                dim * 0.12
              );
            }
          } else if (d.kind === "ring") {
            fillSoft(ctx, p.x, p.y, Math.min(5, d.rx * zoom * 0.18), Math.min(3.5, d.ry * zoom * 0.18), d.rot, 60, 220, 190, dim * 0.4, dim * 0.08);
            fillSoft(ctx, p.x, p.y, 1.2 * zoom, 1.2 * zoom, 0, 255, 255, 230, dim * 0.7, dim * 0.2);
          } else {
            fillSoft(ctx, p.x, p.y, Math.min(5, d.rx * zoom * 0.22), Math.min(3.5, d.ry * zoom * 0.22), d.rot, r0, g0, b0, dim * 0.5, dim * 0.14);
          }
        } else if (d.kind === "spike") {
          ctx.save();
          ctx.translate(p.x, p.y);
          const len = (2.2 + d.mag * 3.2) * zoom;
          const col = (SPEC_RGB[d.spec] || SPEC_RGB.a).cr;
          diffraction(ctx, len, 0.7, col[0], col[1], col[2], dim * 0.7);
          ctx.rotate(Math.PI / 2);
          diffraction(ctx, len * 0.85, 0.6, col[0], col[1], col[2], dim * 0.55);
          ctx.restore();
          fillSoft(ctx, p.x, p.y, 1.1 * zoom, 1.1 * zoom, 0, 255, 255, 255, dim, 0.25);
        } else {
          const s = (0.45 + d.mag * 1.15) * (0.75 + 0.25 * tw) * zoom;
          const pal = SPEC_RGB[d.spec] || (d.warm ? SPEC_RGB.k : SPEC_RGB.a);
          ctx.globalAlpha = dim;
          ctx.fillStyle = "rgb(" + pal.cr[0] + "," + pal.cr[1] + "," + pal.cr[2] + ")";
          ctx.beginPath();
          ctx.arc(p.x, p.y, Math.max(0.35, s), 0, Math.PI * 2);
          ctx.fill();
          ctx.globalAlpha = 1;
        }
      });
    }
  }

  function spriteCanvas(kind, style, opts) {
    const c = document.createElement("canvas");
    c.width = 256;
    c.height = 256;
    const g = c.getContext("2d");
    if (kind === "dir") {
      paintDirNebula(g, 128, 128, 58, {
        style: style || "emission",
        breathe: 1,
        on: !!(opts && opts.on),
        seed: (opts && opts.seed) || hashName(style || "emission"),
      });
    } else {
      paintFileStar(g, 128, 128, 22, {
        twinkle: 1,
        spec: (opts && opts.spec) || style || "a",
        morph: (opts && opts.morph) || "spike4",
        on: !!(opts && opts.on),
        rot: (opts && opts.rot) || 0,
      });
    }
    return c;
  }

  window.SDLook = {
    hashName,
    dirStyle,
    starLook,
    dirKinds: DIR_KINDS,
    twinkleAt,
    paintFileStar,
    paintDirNebula,
    makeFarField,
    drawFarField2d,
    spriteCanvas,
  };
})();
