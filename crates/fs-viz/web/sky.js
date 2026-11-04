(function () {
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

  function boot() {
    const canvas = document.getElementById("sky");
    if (!canvas) return;
    const ctx = canvas.getContext("2d", { alpha: false });
    const farBake = document.createElement("canvas");
    const nearBake = document.createElement("canvas");
    let twinkles = [];
    let bw = 1;
    let bh = 1;
    let cssW = 1;
    let cssH = 1;
    let bakeTimer = 0;

    function soft(g, x, y, rx, ry, ang, r, gv, b, a, aMid) {
      if (rx < 0.5 || ry < 0.5 || a <= 0) return;
      g.save();
      g.translate(x, y);
      g.rotate(ang || 0);
      g.scale(Math.max(0.5, rx), Math.max(0.5, ry));
      const grd = g.createRadialGradient(0, 0, 0, 0, 0, 1);
      grd.addColorStop(0, "rgba(" + r + "," + gv + "," + b + "," + a + ")");
      grd.addColorStop(0.4, "rgba(" + r + "," + gv + "," + b + "," + (aMid != null ? aMid : a * 0.32) + ")");
      grd.addColorStop(1, "rgba(" + r + "," + gv + "," + b + ",0)");
      g.fillStyle = grd;
      g.beginPath();
      g.arc(0, 0, 1, 0, Math.PI * 2);
      g.fill();
      g.restore();
    }

    function spike(g, x, y, len, thick, r, gv, b, a) {
      g.save();
      g.translate(x, y);
      g.scale(len, Math.max(0.45, thick));
      const grd = g.createRadialGradient(0, 0, 0, 0, 0, 1);
      grd.addColorStop(0, "rgba(" + r + "," + gv + "," + b + "," + a + ")");
      grd.addColorStop(0.14, "rgba(" + r + "," + gv + "," + b + "," + a * 0.4 + ")");
      grd.addColorStop(1, "rgba(" + r + "," + gv + "," + b + ",0)");
      g.fillStyle = grd;
      g.beginPath();
      g.arc(0, 0, 1, 0, Math.PI * 2);
      g.fill();
      g.restore();
    }

    function galaxy(g, x, y, rng) {
      const kind = rng();
      const rot = rng() * Math.PI;
      const rx = 5 + rng() * 24;
      const ry = rx * (0.28 + rng() * 0.55);
      const palettes = [
        [255, 150, 90],
        [255, 110, 160],
        [90, 140, 255],
        [180, 170, 220],
        [255, 210, 130],
        [70, 210, 170],
        [120, 200, 255],
        [255, 90, 80],
      ];
      const rgb = palettes[Math.floor(rng() * palettes.length)];
      const a = (0.07 + rng() * 0.11) * 1.8;
      g.save();
      g.globalCompositeOperation = "lighter";
      soft(g, x, y, rx, ry, rot, rgb[0], rgb[1], rgb[2], a, a * 0.28);
      if (kind < 0.16) {
        const R = rx * 0.95;
        const arms = kind < 0.08 ? 3 : 2;
        for (let arm = 0; arm < arms; arm++) {
          for (let i = 0; i < 14; i++) {
            const u = i / 14;
            const ang = rot + arm * ((Math.PI * 2) / arms) + u * 3.2;
            const rr = R * (0.15 + u * 0.9);
            soft(
              g,
              x + Math.cos(ang) * rr,
              y + Math.sin(ang) * rr * (ry / rx),
              R * (0.08 + (1 - u) * 0.06),
              R * 0.05,
              ang,
              rgb[0],
              rgb[1],
              rgb[2],
              a * (0.5 + (1 - u) * 0.4),
              a * 0.1
            );
          }
        }
        soft(g, x, y, rx * 0.22, ry * 0.22, 0, 255, 230, 255, a * 1.4, a * 0.4);
      } else if (kind < 0.28) {
        g.globalCompositeOperation = "source-over";
        soft(g, x, y, rx * 0.15, ry * 0.55, rot, 8, 6, 12, 0.35, 0.08);
      } else if (kind < 0.4) {
        for (let i = 0; i < 14; i++) {
          const ang = (i / 14) * Math.PI * 2;
          soft(g, x + Math.cos(ang) * rx * 0.55, y + Math.sin(ang) * ry * 0.5, rx * 0.12, ry * 0.08, ang, 60, 230, 190, a * 0.7, a * 0.15);
        }
        soft(g, x, y, rx * 0.12, ry * 0.12, 0, 255, 255, 220, a * 1.2, a * 0.3);
      } else if (kind < 0.52) {
        for (let i = 0; i < 12; i++) {
          const ang = rng() * Math.PI * 2;
          const rr = Math.pow(rng(), 0.6) * rx;
          soft(g, x + Math.cos(ang) * rr, y + Math.sin(ang) * rr * 0.85, 1.4, 1.4, 0, 255, 240, 220, a * 0.9, a * 0.2);
        }
      } else if (kind < 0.62) {
        for (let i = 0; i < 5; i++) {
          soft(
            g,
            x + (rng() - 0.5) * rx * 1.4,
            y + (rng() - 0.5) * ry * 1.4,
            rx * (0.25 + rng() * 0.3),
            ry * (0.18 + rng() * 0.25),
            rng() * Math.PI,
            rgb[0],
            rgb[1],
            rgb[2],
            a * 0.7,
            a * 0.15
          );
        }
      }
      g.restore();
    }

    function nebulaBank(g, w, h, rng, cx, cy, palette) {
      g.save();
      g.globalCompositeOperation = "lighter";
      const n = 7 + Math.floor(rng() * 5);
      for (let i = 0; i < n; i++) {
        const pal = palette[i % palette.length];
        soft(
          g,
          cx + (rng() - 0.5) * w * 0.28,
          cy + (rng() - 0.5) * h * 0.24,
          w * (0.12 + rng() * 0.22),
          h * (0.08 + rng() * 0.16),
          rng() * Math.PI,
          pal[0],
          pal[1],
          pal[2],
          pal[3] * (0.55 + rng() * 0.45),
          pal[3] * 0.18
        );
      }
      g.globalCompositeOperation = "source-over";
      for (let i = 0; i < 3; i++) {
        soft(
          g,
          cx + (rng() - 0.5) * w * 0.16,
          cy + (rng() - 0.5) * h * 0.12,
          w * (0.04 + rng() * 0.08),
          h * (0.09 + rng() * 0.1),
          rng() * Math.PI,
          4,
          6,
          12,
          0.18 + rng() * 0.12,
          0.04
        );
      }
      g.restore();
    }

    function paintStars(g, w, h, rng, n, cluster, collectBright) {
      const bright = [];
      for (let i = 0; i < n; i++) {
        let x = rng() * w;
        let y = rng() * h;
        if (cluster && rng() < 0.45) {
          x = cluster.x + (rng() - 0.5) * cluster.s * w;
          y = cluster.y + (rng() - 0.5) * cluster.s * h * 0.85;
        }
        const mag = Math.pow(rng(), 2.4);
        const specs = [
          [155, 185, 255],
          [180, 210, 255],
          [236, 242, 255],
          [255, 248, 225],
          [255, 228, 165],
          [255, 186, 110],
          [255, 130, 90],
          [255, 140, 210],
        ];
        const rgb = specs[Math.floor(rng() * specs.length)];
        const a = 0.1 + mag * 0.75;
        const rad = 0.25 + mag * 1.35;
        g.globalAlpha = a;
        g.fillStyle = "rgb(" + rgb[0] + "," + rgb[1] + "," + rgb[2] + ")";
        g.beginPath();
        g.arc(x, y, rad, 0, Math.PI * 2);
        g.fill();
        g.globalAlpha = 1;
        if (mag > 0.72 && rng() > 0.55) {
          spike(g, x, y, rad * (6 + mag * 8), 0.55 + mag * 0.4, rgb[0], rgb[1], rgb[2], a * 0.55);
          g.save();
          g.translate(x, y);
          g.rotate(Math.PI / 2);
          spike(g, 0, 0, rad * (5.2 + mag * 7), 0.5, rgb[0], rgb[1], rgb[2], a * 0.45);
          g.restore();
          if (collectBright) bright.push({ x: x, y: y, rgb: rgb, mag: mag, p: rng() });
        } else if (mag > 0.55 && collectBright && rng() > 0.7) {
          bright.push({ x: x, y: y, rgb: rgb, mag: mag, p: rng() });
        }
      }
      return bright;
    }

    function rebuildBakes() {
      const pad = 1.24;
      bw = Math.max(64, Math.floor(cssW * pad));
      bh = Math.max(64, Math.floor(cssH * pad));
      farBake.width = bw;
      farBake.height = bh;
      nearBake.width = bw;
      nearBake.height = bh;
      const gf = farBake.getContext("2d");
      const gn = nearBake.getContext("2d");
      gf.setTransform(1, 0, 0, 1, 0, 0);
      gn.setTransform(1, 0, 0, 1, 0, 0);
      gf.clearRect(0, 0, bw, bh);
      gn.clearRect(0, 0, bw, bh);

      const rngF = mulberry(0xc0ffee);
      const rngN = mulberry(0x5eed);

      nebulaBank(gf, bw, bh, rngF, bw * 0.22, bh * 0.2, [
        [170, 40, 130, 0.29],
        [255, 110, 60, 0.18],
        [90, 40, 140, 0.22],
        [40, 70, 160, 0.16],
      ]);
      nebulaBank(gf, bw, bh, rngF, bw * 0.78, bh * 0.74, [
        [40, 80, 180, 0.25],
        [70, 40, 140, 0.18],
        [180, 70, 140, 0.13],
      ]);
      nebulaBank(gf, bw, bh, rngF, bw * 0.52, bh * 0.42, [
        [60, 30, 100, 0.13],
        [30, 50, 120, 0.09],
      ]);
      nebulaBank(gf, bw, bh, rngF, bw * 0.12, bh * 0.72, [
        [40, 180, 140, 0.18],
        [30, 90, 80, 0.13],
        [80, 220, 200, 0.11],
      ]);
      nebulaBank(gf, bw, bh, rngF, bw * 0.88, bh * 0.18, [
        [255, 170, 70, 0.16],
        [255, 90, 50, 0.11],
        [180, 60, 40, 0.09],
      ]);

      gf.save();
      gf.globalCompositeOperation = "lighter";
      for (let i = 0; i < 96; i++) {
        galaxy(gf, rngF() * bw, rngF() * bh, rngF);
      }
      for (let i = 0; i < 28; i++) {
        galaxy(
          gf,
          bw * 0.2 + (rngF() - 0.5) * bw * 0.22,
          bh * 0.18 + (rngF() - 0.5) * bh * 0.18,
          rngF
        );
      }
      gf.restore();
      paintStars(gf, bw, bh, rngF, 780, { x: bw * 0.24, y: bh * 0.2, s: 0.38 }, false);
      paintStars(gf, bw, bh, rngF, 220, null, false);

      nebulaBank(gn, bw, bh, rngN, bw * 0.68, bh * 0.28, [
        [200, 80, 140, 0.11],
        [255, 140, 70, 0.08],
      ]);
      nebulaBank(gn, bw, bh, rngN, bw * 0.3, bh * 0.62, [
        [70, 160, 255, 0.09],
        [40, 200, 170, 0.07],
      ]);
      gn.save();
      gn.globalCompositeOperation = "lighter";
      for (let i = 0; i < 18; i++) galaxy(gn, rngN() * bw, rngN() * bh, rngN);
      gn.restore();
      twinkles = paintStars(gn, bw, bh, rngN, 160, { x: bw * 0.7, y: bh * 0.3, s: 0.3 }, true);
      paintStars(gn, bw, bh, rngN, 90, null, false);
      if (twinkles.length > 56) twinkles = twinkles.slice(0, 56);
    }

    function applySize() {
      const dpr = Math.min(2, window.devicePixelRatio || 1);
      cssW = window.innerWidth || 1;
      cssH = window.innerHeight || 1;
      canvas.width = Math.floor(cssW * dpr);
      canvas.height = Math.floor(cssH * dpr);
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      canvas.style.width = cssW + "px";
      canvas.style.height = cssH + "px";
    }
    function fit() {
      applySize();
      clearTimeout(bakeTimer);
      bakeTimer = setTimeout(rebuildBakes, 80);
    }
    applySize();
    rebuildBakes();
    window.addEventListener("resize", fit);

    window.SDSky = { frozen: false };
    function draw(now) {
      if (window.SDSky && window.SDSky.frozen) {
        requestAnimationFrame(draw);
        return;
      }
      const w = cssW;
      const h = cssH;
      const t = now / 1000;
      ctx.globalCompositeOperation = "source-over";
      const voidG = ctx.createRadialGradient(w * 0.38, h * 0.28, 0, w * 0.5, h * 0.55, Math.max(w, h) * 0.82);
      voidG.addColorStop(0, "#0b1020");
      voidG.addColorStop(0.45, "#05070d");
      voidG.addColorStop(1, "#020309");
      ctx.fillStyle = voidG;
      ctx.fillRect(0, 0, w, h);

      const maxX = Math.max(0, bw - w);
      const maxY = Math.max(0, bh - h);
      const farX = (0.5 + 0.5 * Math.sin(t * 0.017)) * maxX;
      const farY = (0.5 + 0.5 * Math.cos(t * 0.013)) * maxY;
      const nearX = (0.5 + 0.5 * Math.sin(t * 0.031 + 1.2)) * maxX;
      const nearY = (0.5 + 0.5 * Math.cos(t * 0.024 + 0.6)) * maxY;
      const breathe = 0.88 + 0.12 * Math.sin(t * 0.11);

      ctx.globalAlpha = breathe;
      ctx.drawImage(farBake, -farX, -farY);
      ctx.globalAlpha = 0.72;
      ctx.drawImage(nearBake, -nearX, -nearY);
      ctx.globalAlpha = 1;

      ctx.globalCompositeOperation = "lighter";
      for (let i = 0; i < twinkles.length; i++) {
        const s = twinkles[i];
        const x = s.x - nearX;
        const y = s.y - nearY;
        if (x < -8 || y < -8 || x > w + 8 || y > h + 8) continue;
        const tw = 0.45 + 0.55 * Math.sin(t * (0.7 + s.p * 0.9) + s.p * 8);
        const a = (0.2 + s.mag * 0.55) * tw;
        const rad = 0.7 + s.mag * 1.8 * (0.75 + 0.35 * tw);
        ctx.globalAlpha = a;
        ctx.fillStyle = "rgb(" + s.rgb[0] + "," + s.rgb[1] + "," + s.rgb[2] + ")";
        ctx.beginPath();
        ctx.arc(x, y, rad, 0, Math.PI * 2);
        ctx.fill();
      }
      ctx.globalAlpha = 1;

      ctx.globalCompositeOperation = "source-over";
      const vig = ctx.createRadialGradient(w * 0.5, h * 0.48, Math.min(w, h) * 0.25, w * 0.5, h * 0.5, Math.max(w, h) * 0.72);
      vig.addColorStop(0, "rgba(0,0,0,0)");
      vig.addColorStop(1, "rgba(2,3,8,0.42)");
      ctx.fillStyle = vig;
      ctx.fillRect(0, 0, w, h);

      requestAnimationFrame(draw);
    }
    requestAnimationFrame(draw);
    window.SDSky = window.SDSky || { frozen: false };
  }
  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", boot);
  else boot();
})();
