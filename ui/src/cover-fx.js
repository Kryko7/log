// Atmospheric Cinemagraph Canvas Engine for Log animated covers.
// Pairs painted watercolor covers with silky 60/120fps procedural atmospheric layers
// (raindrops on glass, candlelit fireflies, winter lantern snowfall, tumbling autumn
// leaves, golden-hour sunbeams & tea steam, coastal sea mist & lighthouse beacon).
// Automatically pauses when hidden, offscreen, or under prefers-reduced-motion.

const LEGACY_ANIM_MAP = {
  "/assets/covers/anim-rainy-window.svg": {
    fx: "rain",
    backdrop: "/assets/covers/rainy-tea-desk.png",
  },
  "/assets/covers/anim-firefly-meadow.svg": {
    fx: "fireflies",
    backdrop: "/assets/covers/midnight-ink-garden.png",
  },
  "/assets/covers/anim-autumn-drift.svg": {
    fx: "autumn",
    backdrop: "/assets/covers/autumn-maple-letter.png",
  },
  "/assets/covers/anim-ink-bloom.svg": {
    fx: "snow",
    backdrop: "/assets/covers/winter-pine-lantern.png",
  },
  "/assets/covers/anim-coastal-tide.svg": {
    fx: "coastal",
    backdrop: "/assets/covers/coastal-lighthouse-sketch.png",
  },
  "/assets/covers/anim-morning-steam.svg": {
    fx: "sunbeams",
    backdrop: "/assets/covers/golden-hour-window.png",
  },
};

const VALID_FX = new Set([
  "rain",
  "fireflies",
  "snow",
  "autumn",
  "sunbeams",
  "coastal",
]);

/**
 * Resolve an entry/notebook cover asset path into its static image URL and optional live FX key.
 * Supports both `#fx=<name>` paths and legacy `/assets/covers/anim-*.svg` paths.
 * @param {string|null|undefined} assetPath
 * @returns {{ basePath: string, fx: string|null }}
 */
export function parseCoverAsset(assetPath) {
  const raw = String(assetPath || "").trim();
  if (!raw) return { basePath: "", fx: null };

  const legacy = LEGACY_ANIM_MAP[raw];
  if (legacy) {
    return { basePath: legacy.backdrop, fx: legacy.fx };
  }

  const hashIdx = raw.indexOf("#");
  if (hashIdx >= 0) {
    const basePath = raw.slice(0, hashIdx);
    const hash = raw.slice(hashIdx + 1);
    const m = /(?:^|&)fx=([a-z0-9_-]+)/i.exec(hash);
    const fx = m && VALID_FX.has(m[1].toLowerCase()) ? m[1].toLowerCase() : null;
    return { basePath, fx };
  }

  return { basePath: raw, fx: null };
}

function prefersReducedMotion() {
  return (
    typeof window !== "undefined" &&
    typeof window.matchMedia === "function" &&
    window.matchMedia("(prefers-reduced-motion: reduce)").matches
  );
}

// Deterministic seeded PRNG so each scene opens with balanced artistic framing
function mulberry32(seed) {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

function drawMapleLeaf(ctx, size) {
  ctx.beginPath();
  ctx.moveTo(0, -size);
  ctx.lineTo(size * 0.24, -size * 0.36);
  ctx.lineTo(size * 0.92, -size * 0.52);
  ctx.lineTo(size * 0.48, -size * 0.04);
  ctx.lineTo(size * 0.95, size * 0.32);
  ctx.lineTo(size * 0.34, size * 0.36);
  ctx.lineTo(size * 0.42, size * 0.78);
  ctx.lineTo(0, size * 0.46);
  ctx.lineTo(-size * 0.42, size * 0.78);
  ctx.lineTo(-size * 0.34, size * 0.36);
  ctx.lineTo(-size * 0.95, size * 0.32);
  ctx.lineTo(-size * 0.48, -size * 0.04);
  ctx.lineTo(-size * 0.92, -size * 0.52);
  ctx.lineTo(-size * 0.24, -size * 0.36);
  ctx.closePath();
  ctx.fill();
  ctx.stroke();
}

function drawGinkgoLeaf(ctx, size) {
  ctx.beginPath();
  ctx.moveTo(0, size * 0.65);
  ctx.bezierCurveTo(
    -size * 0.65,
    size * 0.15,
    -size * 1.12,
    -size * 0.42,
    -size * 0.76,
    -size * 0.85
  );
  ctx.quadraticCurveTo(-size * 0.22, -size * 1.02, 0, -size * 0.45);
  ctx.quadraticCurveTo(size * 0.22, -size * 1.02, size * 0.76, -size * 0.85);
  ctx.bezierCurveTo(
    size * 1.12,
    -size * 0.42,
    size * 0.65,
    size * 0.15,
    0,
    size * 0.65
  );
  ctx.closePath();
  ctx.fill();
  ctx.stroke();
}

function createRainRenderer(rand) {
  const streaks = Array.from({ length: 74 }, (_, i) => {
    const depth = i < 24 ? 0.45 : i < 54 ? 0.78 : 1.15;
    return {
      x: rand(),
      y: rand(),
      len: (16 + rand() * 24) * depth,
      speed: (0.42 + rand() * 0.34) * depth,
      alpha: (0.14 + rand() * 0.22) * depth,
      width: 0.7 + depth * 0.65,
      depth,
    };
  });

  const drops = Array.from({ length: 18 }, () => ({
    x: 0.05 + rand() * 0.9,
    y: rand(),
    r: 1.9 + rand() * 3.1,
    speed: 0.008 + rand() * 0.022,
    wobblePhase: rand() * Math.PI * 2,
    stallTimer: rand() * 4,
    moving: rand() > 0.45,
    trail: [],
  }));

  return (ctx, w, h, dt, t, pointer) => {
    // Subtle cool overcast glass tint
    const tint = ctx.createLinearGradient(0, 0, 0, h);
    tint.addColorStop(0, "rgba(42, 58, 52, 0.12)");
    tint.addColorStop(0.6, "rgba(251, 247, 240, 0.02)");
    tint.addColorStop(1, "rgba(38, 32, 29, 0.10)");
    ctx.fillStyle = tint;
    ctx.fillRect(0, 0, w, h);

    // Multi-depth falling rain streaks
    const windX = -0.19 + pointer.x * 0.04;
    ctx.lineCap = "round";
    for (const s of streaks) {
      s.y += s.speed * dt;
      s.x += windX * s.speed * dt * (h / Math.max(w, 1));
      if (s.y > 1.12) {
        s.y = -0.12;
        s.x = rand() * 1.18;
      }
      if (s.x < -0.1) s.x += 1.2;

      const px = s.x * w + pointer.x * 6 * s.depth;
      const py = s.y * h;
      const dx = windX * s.len;
      const dy = s.len;

      const grad = ctx.createLinearGradient(px, py, px + dx, py + dy);
      grad.addColorStop(0, "rgba(251, 247, 240, 0)");
      grad.addColorStop(0.75, `rgba(235, 243, 238, ${s.alpha.toFixed(3)})`);
      grad.addColorStop(1, `rgba(255, 253, 248, ${(s.alpha * 1.45).toFixed(3)})`);
      ctx.strokeStyle = grad;
      ctx.lineWidth = s.width;
      ctx.beginPath();
      ctx.moveTo(px, py);
      ctx.lineTo(px + dx, py + dy);
      ctx.stroke();
    }

    // Glistening refractive raindrops clinging and sliding down glass
    for (const d of drops) {
      d.stallTimer -= dt;
      if (d.stallTimer <= 0) {
        d.moving = !d.moving;
        d.stallTimer = d.moving ? 1.4 + rand() * 2.8 : 0.9 + rand() * 2.4;
      }
      if (d.moving) {
        const prevX = d.x;
        const prevY = d.y;
        d.y += d.speed * dt * (1.2 + Math.sin(t * 2.2 + d.wobblePhase) * 0.35);
        d.x += Math.sin(t * 1.7 + d.wobblePhase) * 0.0005;
        if (Math.hypot((d.x - prevX) * w, (d.y - prevY) * h) > 1.8) {
          d.trail.push({ x: d.x, y: d.y, a: 0.32, r: d.r * 0.42 });
          if (d.trail.length > 10) d.trail.shift();
        }
      }
      if (d.y > 1.06) {
        d.y = -0.04;
        d.x = 0.05 + rand() * 0.9;
        d.trail.length = 0;
      }

      // Fading condensation trail
      for (let i = d.trail.length - 1; i >= 0; i -= 1) {
        const tr = d.trail[i];
        tr.a -= dt * 0.085;
        if (tr.a <= 0.01) {
          d.trail.splice(i, 1);
          continue;
        }
        ctx.fillStyle = `rgba(251, 247, 240, ${tr.a.toFixed(3)})`;
        ctx.beginPath();
        ctx.arc(tr.x * w + pointer.x * 8, tr.y * h, tr.r, 0, Math.PI * 2);
        ctx.fill();
      }

      const cx = d.x * w + pointer.x * 8;
      const cy = d.y * h;
      // Soft drop shadow beneath glass droplet
      ctx.fillStyle = "rgba(38, 32, 29, 0.22)";
      ctx.beginPath();
      ctx.ellipse(cx + 0.6, cy + 1.4, d.r * 0.92, d.r * 1.15, 0, 0, Math.PI * 2);
      ctx.fill();

      // Translucent droplet lens
      const lens = ctx.createRadialGradient(
        cx - d.r * 0.25,
        cy - d.r * 0.3,
        0.2,
        cx,
        cy,
        d.r * 1.2
      );
      lens.addColorStop(0, "rgba(255, 253, 248, 0.72)");
      lens.addColorStop(0.55, "rgba(232, 240, 234, 0.28)");
      lens.addColorStop(1, "rgba(84, 110, 90, 0.14)");
      ctx.fillStyle = lens;
      ctx.beginPath();
      ctx.ellipse(cx, cy, d.r * 0.88, d.r * 1.18, 0, 0, Math.PI * 2);
      ctx.fill();

      // Specular crescent pinpoint
      ctx.fillStyle = "rgba(255, 255, 255, 0.85)";
      ctx.beginPath();
      ctx.arc(cx - d.r * 0.28, cy - d.r * 0.35, Math.max(0.7, d.r * 0.26), 0, Math.PI * 2);
      ctx.fill();
    }
  };
}

function createFirefliesRenderer(rand) {
  const stars = Array.from({ length: 20 }, () => ({
    x: rand(),
    y: 0.05 + rand() * 0.52,
    r: 0.7 + rand() * 1.1,
    phase: rand() * Math.PI * 2,
    speed: 0.8 + rand() * 1.4,
  }));

  const flies = Array.from({ length: 28 }, (_, i) => {
    const depth = i < 10 ? 0.55 : i < 21 ? 0.9 : 1.3;
    return {
      bx: 0.06 + rand() * 0.88,
      by: 0.14 + rand() * 0.76,
      ax: (0.025 + rand() * 0.055) * depth,
      ay: (0.04 + rand() * 0.09) * depth,
      fx: 0.22 + rand() * 0.34,
      fy: 0.26 + rand() * 0.38,
      px: rand() * Math.PI * 2,
      py: rand() * Math.PI * 2,
      pulseSpeed: 0.65 + rand() * 1.1,
      pulsePhase: rand() * Math.PI * 2,
      radius: (10 + rand() * 14) * depth,
      core: (1.2 + rand() * 1.4) * depth,
      depth,
      ox: 0,
      oy: 0,
    };
  });

  return (ctx, w, h, dt, t, pointer) => {
    // Subtle twinkling stars in upper canopy
    for (const s of stars) {
      const a = 0.16 + 0.58 * (0.5 + 0.5 * Math.sin(t * s.speed + s.phase));
      ctx.fillStyle = `rgba(247, 236, 212, ${a.toFixed(3)})`;
      ctx.beginPath();
      ctx.arc(s.x * w + pointer.x * 2, s.y * h + pointer.y * 1.5, s.r, 0, Math.PI * 2);
      ctx.fill();
    }

    // Warm additive firefly glows
    ctx.save();
    ctx.globalCompositeOperation = "lighter";
    for (const f of flies) {
      const baseX = (f.bx + Math.sin(t * f.fx + f.px) * f.ax) * w;
      const baseY = (f.by + Math.cos(t * f.fy + f.py) * f.ay) * h;

      // Gentle cursor attraction when pointer is active
      if (pointer.active) {
        const tx = pointer.px - baseX;
        const ty = pointer.py - baseY;
        const dist = Math.hypot(tx, ty);
        const pull = dist < 190 ? (1 - dist / 190) * 18 * f.depth : 0;
        const goalX = dist > 1 ? (tx / dist) * pull : 0;
        const goalY = dist > 1 ? (ty / dist) * pull : 0;
        f.ox += (goalX - f.ox) * Math.min(1, dt * 2.4);
        f.oy += (goalY - f.oy) * Math.min(1, dt * 2.4);
      } else {
        f.ox *= Math.max(0, 1 - dt * 1.8);
        f.oy *= Math.max(0, 1 - dt * 1.8);
      }

      const x = baseX + f.ox + pointer.x * 8 * f.depth;
      const y = baseY + f.oy + pointer.y * 5 * f.depth;

      // Natural organic firefly blink envelope
      const wave = Math.sin(t * f.pulseSpeed + f.pulsePhase);
      const glow = Math.pow(Math.max(0, wave * 0.65 + 0.35), 1.7);
      if (glow < 0.03) continue;

      const r = f.radius * (0.72 + glow * 0.45);
      const grad = ctx.createRadialGradient(x, y, 0, x, y, r);
      grad.addColorStop(0, `rgba(255, 246, 214, ${(0.9 * glow).toFixed(3)})`);
      grad.addColorStop(0.28, `rgba(240, 186, 102, ${(0.48 * glow).toFixed(3)})`);
      grad.addColorStop(0.65, `rgba(217, 155, 82, ${(0.16 * glow).toFixed(3)})`);
      grad.addColorStop(1, "rgba(217, 155, 82, 0)");
      ctx.fillStyle = grad;
      ctx.beginPath();
      ctx.arc(x, y, r, 0, Math.PI * 2);
      ctx.fill();

      ctx.fillStyle = `rgba(255, 250, 232, ${(0.95 * glow).toFixed(3)})`;
      ctx.beginPath();
      ctx.arc(x, y, f.core * (0.8 + glow * 0.3), 0, Math.PI * 2);
      ctx.fill();
    }
    ctx.restore();
  };
}

function createSnowRenderer(rand) {
  const flakes = Array.from({ length: 58 }, (_, i) => {
    const depth = i < 24 ? 0.5 : i < 46 ? 0.9 : 1.55;
    return {
      x: rand(),
      y: rand(),
      r: (1.1 + rand() * 2.1) * depth,
      speed: (0.045 + rand() * 0.065) * depth,
      swayAmp: (0.012 + rand() * 0.025) * depth,
      swayFreq: 0.45 + rand() * 0.75,
      phase: rand() * Math.PI * 2,
      alpha: depth > 1.2 ? 0.36 + rand() * 0.22 : 0.45 + rand() * 0.4,
      depth,
    };
  });

  return (ctx, w, h, dt, t, pointer) => {
    // Warm flickering brass lantern glow (aligned with winter-pine-lantern composition)
    const lx = w * 0.65 + pointer.x * 4;
    const ly = h * 0.56 + pointer.y * 3;
    const flicker =
      0.86 +
      0.08 * Math.sin(t * 4.7) +
      0.04 * Math.sin(t * 11.3) +
      0.03 * Math.cos(t * 7.1);
    const haloR = Math.max(w, h) * 0.28 * (0.96 + flicker * 0.06);

    ctx.save();
    ctx.globalCompositeOperation = "lighter";
    const glow = ctx.createRadialGradient(lx, ly, 4, lx, ly, haloR);
    glow.addColorStop(0, `rgba(255, 224, 152, ${(0.26 * flicker).toFixed(3)})`);
    glow.addColorStop(0.4, `rgba(217, 155, 82, ${(0.12 * flicker).toFixed(3)})`);
    glow.addColorStop(1, "rgba(217, 155, 82, 0)");
    ctx.fillStyle = glow;
    ctx.beginPath();
    ctx.arc(lx, ly, haloR, 0, Math.PI * 2);
    ctx.fill();
    ctx.restore();

    // 3-depth softly lit snowfall
    for (const f of flakes) {
      f.y += f.speed * dt;
      f.x += Math.sin(t * f.swayFreq + f.phase) * f.swayAmp * dt + pointer.x * 0.008 * dt;
      if (f.y > 1.08) {
        f.y = -0.06;
        f.x = rand();
      }
      if (f.x < -0.05) f.x += 1.1;
      if (f.x > 1.05) f.x -= 1.1;

      const px = f.x * w + pointer.x * 9 * f.depth;
      const py = f.y * h + pointer.y * 4 * f.depth;
      // Snowflakes passing near the amber lantern catch a warm golden rim tint
      const distLantern = Math.hypot(px - lx, py - ly);
      const warmMix = Math.max(0, 1 - distLantern / (haloR * 1.1));
      const rCol = Math.round(244 + 11 * warmMix);
      const gCol = Math.round(240 - 18 * warmMix);
      const bCol = Math.round(234 - 66 * warmMix);

      if (f.depth > 1.2) {
        const bokeh = ctx.createRadialGradient(px, py, 0, px, py, f.r * 1.6);
        bokeh.addColorStop(0, `rgba(${rCol}, ${gCol}, ${bCol}, ${f.alpha.toFixed(3)})`);
        bokeh.addColorStop(1, `rgba(${rCol}, ${gCol}, ${bCol}, 0)`);
        ctx.fillStyle = bokeh;
        ctx.beginPath();
        ctx.arc(px, py, f.r * 1.6, 0, Math.PI * 2);
        ctx.fill();
      } else {
        ctx.fillStyle = `rgba(${rCol}, ${gCol}, ${bCol}, ${f.alpha.toFixed(3)})`;
        ctx.beginPath();
        ctx.arc(px, py, f.r, 0, Math.PI * 2);
        ctx.fill();
      }
    }
  };
}

function createAutumnRenderer(rand) {
  const palette = [
    { fill: "rgba(181, 86, 62, 0.82)", stroke: "rgba(92, 42, 30, 0.55)" },
    { fill: "rgba(214, 152, 74, 0.84)", stroke: "rgba(116, 78, 30, 0.5)" },
    { fill: "rgba(164, 62, 52, 0.78)", stroke: "rgba(84, 30, 26, 0.5)" },
    { fill: "rgba(196, 118, 66, 0.80)", stroke: "rgba(98, 56, 30, 0.5)" },
    { fill: "rgba(104, 126, 98, 0.68)", stroke: "rgba(56, 74, 54, 0.45)" },
  ];

  const leaves = Array.from({ length: 15 }, (_, i) => {
    const col = palette[i % palette.length];
    return {
      x: rand(),
      y: rand(),
      vx: 0.022 + rand() * 0.032,
      vy: 0.042 + rand() * 0.055,
      size: 6.5 + rand() * 8.5,
      rot: rand() * Math.PI * 2,
      rotSpeed: (rand() - 0.5) * 1.15,
      tumblePhase: rand() * Math.PI * 2,
      tumbleSpeed: 1.0 + rand() * 1.3,
      swayPhase: rand() * Math.PI * 2,
      kind: i % 3 === 0 ? "ginkgo" : "maple",
      fill: col.fill,
      stroke: col.stroke,
    };
  });

  return (ctx, w, h, dt, t, pointer) => {
    // Slow warm golden sheen drifting across handmade cream paper
    const sweepX = ((t * 0.045) % 1.8) - 0.4;
    const sheen = ctx.createLinearGradient(
      w * (sweepX - 0.25),
      0,
      w * (sweepX + 0.25),
      h
    );
    sheen.addColorStop(0, "rgba(255, 245, 222, 0)");
    sheen.addColorStop(0.5, "rgba(255, 241, 206, 0.14)");
    sheen.addColorStop(1, "rgba(255, 245, 222, 0)");
    ctx.fillStyle = sheen;
    ctx.fillRect(0, 0, w, h);

    for (const l of leaves) {
      l.x += (l.vx + Math.sin(t * 0.9 + l.swayPhase) * 0.014 + pointer.x * 0.015) * dt;
      l.y += (l.vy + Math.cos(t * 1.1 + l.swayPhase) * 0.012) * dt;
      l.rot += l.rotSpeed * dt;
      if (l.y > 1.14 || l.x > 1.14) {
        if (rand() > 0.4) {
          l.x = -0.08 + rand() * 0.85;
          l.y = -0.12;
        } else {
          l.x = -0.1;
          l.y = rand() * 0.65;
        }
      }

      const px = l.x * w + pointer.x * 10;
      const py = l.y * h + pointer.y * 5;
      const flutter = 0.42 + 0.58 * Math.abs(Math.cos(t * l.tumbleSpeed + l.tumblePhase));

      // Soft ambient shadow cast onto the paper beneath the leaf
      ctx.save();
      ctx.translate(px + 5, py + 7);
      ctx.rotate(l.rot);
      ctx.scale(1, flutter);
      ctx.fillStyle = "rgba(38, 32, 29, 0.11)";
      ctx.strokeStyle = "transparent";
      if (l.kind === "ginkgo") drawGinkgoLeaf(ctx, l.size);
      else drawMapleLeaf(ctx, l.size);
      ctx.restore();

      // Painted leaf
      ctx.save();
      ctx.translate(px, py);
      ctx.rotate(l.rot);
      ctx.scale(1, flutter);
      ctx.fillStyle = l.fill;
      ctx.strokeStyle = l.stroke;
      ctx.lineWidth = 0.85;
      if (l.kind === "ginkgo") drawGinkgoLeaf(ctx, l.size);
      else drawMapleLeaf(ctx, l.size);
      ctx.restore();
    }
  };
}

function createSunbeamsRenderer(rand) {
  const motes = Array.from({ length: 48 }, () => ({
    x: 0.12 + rand() * 0.78,
    y: rand(),
    r: 0.9 + rand() * 2.0,
    vx: (rand() - 0.35) * 0.012,
    vy: -0.008 - rand() * 0.014,
    phase: rand() * Math.PI * 2,
    speed: 0.7 + rand() * 1.1,
  }));

  const steamWisps = Array.from({ length: 4 }, (_, i) => ({
    offset: (i - 1.5) * 0.014,
    phase: i * 1.6,
    speed: 0.55 + i * 0.09,
    width: 6 + i * 2.2,
  }));

  return (ctx, w, h, dt, t, pointer) => {
    // Volumetric slanting late-afternoon golden sunbeams
    ctx.save();
    ctx.globalCompositeOperation = "screen";
    const beams = [
      { x0: 0.12, w0: 0.2, drift: 0.45, alpha: 0.22 },
      { x0: 0.36, w0: 0.16, drift: 0.62, alpha: 0.18 },
      { x0: 0.54, w0: 0.18, drift: 0.38, alpha: 0.15 },
    ];
    for (const b of beams) {
      const pulse = 0.76 + 0.24 * Math.sin(t * b.drift + b.x0 * 9);
      const sx = (b.x0 + pointer.x * 0.02) * w;
      const bw = b.w0 * w;
      const grad = ctx.createLinearGradient(sx, 0, sx + w * 0.28, h);
      grad.addColorStop(0, `rgba(255, 244, 208, ${(b.alpha * pulse).toFixed(3)})`);
      grad.addColorStop(0.55, `rgba(242, 206, 134, ${(b.alpha * 0.55 * pulse).toFixed(3)})`);
      grad.addColorStop(1, "rgba(217, 155, 82, 0)");
      ctx.fillStyle = grad;
      ctx.beginPath();
      ctx.moveTo(sx, 0);
      ctx.lineTo(sx + bw, 0);
      ctx.lineTo(sx + bw + w * 0.28, h);
      ctx.lineTo(sx + w * 0.22, h);
      ctx.closePath();
      ctx.fill();
    }
    ctx.restore();

    // Organic rising curling teacup steam wisps (aligned with golden-hour-window teacup)
    const cupX = w * 0.59 + pointer.x * 6;
    const cupY = h * 0.64 + pointer.y * 3;
    ctx.save();
    ctx.lineCap = "round";
    for (const sw of steamWisps) {
      const cycle = (t * sw.speed + sw.phase) % 3.2;
      const progress = cycle / 3.2;
      const fade = Math.sin(progress * Math.PI);
      if (fade <= 0.02) continue;

      const xBase = cupX + sw.offset * w;
      const riseH = h * (0.34 + progress * 0.22);
      const sway1 = Math.sin(t * 1.4 + sw.phase) * 14;
      const sway2 = Math.cos(t * 1.1 + sw.phase) * 18;

      const grad = ctx.createLinearGradient(xBase, cupY, xBase, cupY - riseH);
      grad.addColorStop(0, "rgba(255, 250, 240, 0)");
      grad.addColorStop(0.35, `rgba(255, 248, 234, ${(0.22 * fade).toFixed(3)})`);
      grad.addColorStop(0.75, `rgba(255, 248, 234, ${(0.12 * fade).toFixed(3)})`);
      grad.addColorStop(1, "rgba(255, 248, 234, 0)");

      ctx.strokeStyle = grad;
      ctx.lineWidth = sw.width * (0.7 + progress * 0.7);
      ctx.beginPath();
      ctx.moveTo(xBase, cupY - progress * 12);
      ctx.bezierCurveTo(
        xBase + sway1,
        cupY - riseH * 0.35,
        xBase - sway2,
        cupY - riseH * 0.72,
        xBase + sway1 * 0.7 + 8,
        cupY - riseH
      );
      ctx.stroke();
    }
    ctx.restore();

    // Sunlit floating dust motes
    ctx.save();
    ctx.globalCompositeOperation = "lighter";
    for (const m of motes) {
      m.x += (m.vx + Math.sin(t * 0.6 + m.phase) * 0.005) * dt;
      m.y += m.vy * dt;
      if (m.y < -0.05) {
        m.y = 1.05;
        m.x = 0.1 + rand() * 0.8;
      }
      if (m.x < 0) m.x = 1;
      if (m.x > 1) m.x = 0;

      const shimmer = 0.22 + 0.68 * (0.5 + 0.5 * Math.sin(t * m.speed + m.phase));
      const px = m.x * w + pointer.x * 8;
      const py = m.y * h + pointer.y * 4;

      const g = ctx.createRadialGradient(px, py, 0, px, py, m.r * 2.8);
      g.addColorStop(0, `rgba(255, 247, 220, ${(0.85 * shimmer).toFixed(3)})`);
      g.addColorStop(1, "rgba(240, 200, 120, 0)");
      ctx.fillStyle = g;
      ctx.beginPath();
      ctx.arc(px, py, m.r * 2.8, 0, Math.PI * 2);
      ctx.fill();
    }
    ctx.restore();
  };
}

function createCoastalRenderer(rand) {
  const mistBands = Array.from({ length: 6 }, (_, i) => ({
    x: rand(),
    y: 0.34 + i * 0.065,
    rx: 0.24 + rand() * 0.22,
    ry: 0.055 + rand() * 0.045,
    speed: (0.012 + rand() * 0.016) * (i % 2 === 0 ? 1 : -0.7),
    alpha: 0.14 + rand() * 0.12,
  }));

  const glints = Array.from({ length: 24 }, () => ({
    x: 0.2 + rand() * 0.72,
    y: 0.56 + rand() * 0.22,
    len: 8 + rand() * 20,
    phase: rand() * Math.PI * 2,
    speed: 1.0 + rand() * 1.4,
  }));

  const gulls = [
    { x: 0.62, y: 0.24, speed: 0.016, scale: 1.0, phase: 0 },
    { x: 0.71, y: 0.19, speed: 0.012, scale: 0.72, phase: 1.8 },
  ];

  return (ctx, w, h, dt, t, pointer) => {
    // Distant headland lighthouse beacon pulse (aligned with coastal-lighthouse-sketch)
    const bx = w * 0.372 + pointer.x * 3;
    const by = h * 0.295 + pointer.y * 2;
    const sweep = 0.35 + 0.65 * Math.pow(0.5 + 0.5 * Math.sin(t * 1.25), 2.2);

    ctx.save();
    ctx.globalCompositeOperation = "screen";
    // Soft sweeping beam cone
    const angle = -0.18 + Math.sin(t * 0.62) * 0.22;
    ctx.translate(bx, by);
    ctx.rotate(angle);
    const beamLen = w * 0.22;
    const cone = ctx.createLinearGradient(0, 0, beamLen, 0);
    cone.addColorStop(0, `rgba(255, 236, 178, ${(0.34 * sweep).toFixed(3)})`);
    cone.addColorStop(1, "rgba(255, 236, 178, 0)");
    ctx.fillStyle = cone;
    ctx.beginPath();
    ctx.moveTo(0, -2);
    ctx.lineTo(beamLen, -24);
    ctx.lineTo(beamLen, 24);
    ctx.lineTo(0, 2);
    ctx.closePath();
    ctx.fill();
    ctx.restore();

    const orb = ctx.createRadialGradient(bx, by, 0, bx, by, 22);
    orb.addColorStop(0, `rgba(255, 249, 224, ${(0.9 * sweep).toFixed(3)})`);
    orb.addColorStop(0.4, `rgba(232, 188, 108, ${(0.38 * sweep).toFixed(3)})`);
    orb.addColorStop(1, "rgba(232, 188, 108, 0)");
    ctx.fillStyle = orb;
    ctx.beginPath();
    ctx.arc(bx, by, 22, 0, Math.PI * 2);
    ctx.fill();

    // Rolling translucent coastal watercolor sea mist
    for (const m of mistBands) {
      m.x += m.speed * dt;
      if (m.x > 1.35) m.x = -0.35;
      if (m.x < -0.35) m.x = 1.35;

      const cx = m.x * w + pointer.x * 7;
      const cy = m.y * h + pointer.y * 3;
      const rx = m.rx * w;
      const ry = m.ry * h;
      ctx.save();
      ctx.translate(cx, cy);
      ctx.scale(1, ry / Math.max(rx, 1));
      const g = ctx.createRadialGradient(0, 0, 0, 0, 0, rx);
      g.addColorStop(0, `rgba(248, 246, 239, ${m.alpha.toFixed(3)})`);
      g.addColorStop(1, "rgba(248, 246, 239, 0)");
      ctx.fillStyle = g;
      ctx.beginPath();
      ctx.arc(0, 0, rx, 0, Math.PI * 2);
      ctx.fill();
      ctx.restore();
    }

    // Subtle shimmering water glints
    ctx.lineCap = "round";
    for (const g of glints) {
      const a = 0.08 + 0.34 * Math.pow(0.5 + 0.5 * Math.sin(t * g.speed + g.phase), 2.5);
      const gx = g.x * w + Math.sin(t * 0.5 + g.phase) * 5 + pointer.x * 5;
      const gy = g.y * h;
      ctx.strokeStyle = `rgba(255, 252, 244, ${a.toFixed(3)})`;
      ctx.lineWidth = 1.3;
      ctx.beginPath();
      ctx.moveTo(gx - g.len * 0.5, gy);
      ctx.lineTo(gx + g.len * 0.5, gy);
      ctx.stroke();
    }

    // Distant minimalist ink seabirds gliding slowly
    ctx.strokeStyle = "rgba(82, 74, 66, 0.52)";
    ctx.lineWidth = 1.25;
    for (const bird of gulls) {
      bird.x -= bird.speed * dt;
      if (bird.x < -0.08) bird.x = 1.08;
      const px = bird.x * w + pointer.x * 4;
      const py = bird.y * h + Math.sin(t * 1.3 + bird.phase) * 5;
      const flap = Math.sin(t * 3.4 + bird.phase) * 3.2 * bird.scale;
      const span = 10 * bird.scale;
      ctx.beginPath();
      ctx.moveTo(px - span, py + flap);
      ctx.quadraticCurveTo(px - span * 0.45, py - 3.5 * bird.scale, px, py);
      ctx.quadraticCurveTo(px + span * 0.45, py - 3.5 * bird.scale, px + span, py + flap);
      ctx.stroke();
    }
  };
}

function buildScene(fxName) {
  const seedMap = {
    rain: 104729,
    fireflies: 224737,
    snow: 350377,
    autumn: 479909,
    sunbeams: 611953,
    coastal: 746773,
  };
  const rand = mulberry32(seedMap[fxName] || 1337);
  switch (fxName) {
    case "rain":
      return createRainRenderer(rand);
    case "fireflies":
      return createFirefliesRenderer(rand);
    case "snow":
      return createSnowRenderer(rand);
    case "autumn":
      return createAutumnRenderer(rand);
    case "sunbeams":
      return createSunbeamsRenderer(rand);
    case "coastal":
      return createCoastalRenderer(rand);
    default:
      return null;
  }
}

/**
 * Mount a live atmospheric Canvas cinemagraph layer inside a cover banner container.
 * Gracefully no-ops in headless/DOM-shim environments or under prefers-reduced-motion.
 * @param {HTMLElement} bannerEl
 * @param {string} fxName
 * @param {HTMLImageElement|null} [backdropImg]
 * @returns {{ destroy: () => void }}
 */
export function mountCoverCinemagraph(bannerEl, fxName, backdropImg = null) {
  const noop = { destroy() {} };
  if (!bannerEl || !fxName || !VALID_FX.has(fxName)) return noop;

  const canvas = document.createElement("canvas");
  canvas.className = "entry-cover-fx-canvas";
  canvas.setAttribute("aria-hidden", "true");
  canvas.dataset.coverFx = fxName;
  bannerEl.appendChild(canvas);

  if (typeof canvas.getContext !== "function" || prefersReducedMotion()) {
    return {
      destroy() {
        canvas.remove();
      },
    };
  }

  const ctx = canvas.getContext("2d");
  if (!ctx) {
    return {
      destroy() {
        canvas.remove();
      },
    };
  }

  const drawScene = buildScene(fxName);
  if (!drawScene) {
    return {
      destroy() {
        canvas.remove();
      },
    };
  }

  const ac = new AbortController();
  let destroyed = false;
  let rafId = 0;
  let visible = true;
  let width = 0;
  let height = 0;
  let dpr = 1;
  let lastTs = 0;
  let elapsed = 0;

  const pointer = {
    x: 0,
    y: 0,
    targetX: 0,
    targetY: 0,
    px: 0,
    py: 0,
    active: false,
  };

  function resize() {
    if (destroyed) return;
    const rect = bannerEl.getBoundingClientRect();
    const w = Math.max(1, Math.round(rect.width || 960));
    const h = Math.max(1, Math.round(rect.height || 190));
    const nextDpr = Math.min(window.devicePixelRatio || 1, 2);
    if (w === width && h === height && nextDpr === dpr) return;
    width = w;
    height = h;
    dpr = nextDpr;
    canvas.width = Math.round(w * dpr);
    canvas.height = Math.round(h * dpr);
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  }

  function applyParallax() {
    if (!backdropImg) return;
    const tx = (-pointer.x * 6).toFixed(2);
    const ty = (-pointer.y * 3.5).toFixed(2);
    backdropImg.style.setProperty("--cover-px", `${tx}px`);
    backdropImg.style.setProperty("--cover-py", `${ty}px`);
  }

  bannerEl.addEventListener(
    "pointermove",
    (ev) => {
      const rect = bannerEl.getBoundingClientRect();
      if (rect.width <= 0 || rect.height <= 0) return;
      pointer.px = ev.clientX - rect.left;
      pointer.py = ev.clientY - rect.top;
      pointer.targetX = ((pointer.px / rect.width) - 0.5) * 2;
      pointer.targetY = ((pointer.py / rect.height) - 0.5) * 2;
      pointer.active = true;
    },
    { passive: true, signal: ac.signal }
  );

  bannerEl.addEventListener(
    "pointerleave",
    () => {
      pointer.targetX = 0;
      pointer.targetY = 0;
      pointer.active = false;
    },
    { passive: true, signal: ac.signal }
  );

  let resizeObs = null;
  if (typeof ResizeObserver === "function") {
    resizeObs = new ResizeObserver(resize);
    resizeObs.observe(bannerEl);
  } else {
    window.addEventListener("resize", resize, { passive: true, signal: ac.signal });
  }

  let interObs = null;
  if (typeof IntersectionObserver === "function") {
    interObs = new IntersectionObserver(
      (entries) => {
        visible = Boolean(entries[0]?.isIntersecting);
        if (visible && !document.hidden && !rafId && !destroyed) {
          lastTs = 0;
          rafId = requestAnimationFrame(tick);
        }
      },
      { threshold: 0.02 }
    );
    interObs.observe(bannerEl);
  }

  document.addEventListener(
    "visibilitychange",
    () => {
      if (!document.hidden && visible && !rafId && !destroyed) {
        lastTs = 0;
        rafId = requestAnimationFrame(tick);
      }
    },
    { signal: ac.signal }
  );

  function tick(ts) {
    rafId = 0;
    if (destroyed || !canvas.isConnected) {
      destroy();
      return;
    }
    if (document.hidden || !visible) return;

    const dt = lastTs ? Math.min((ts - lastTs) / 1000, 0.05) : 1 / 60;
    lastTs = ts;
    elapsed += dt;

    pointer.x += (pointer.targetX - pointer.x) * Math.min(1, dt * 5.5);
    pointer.y += (pointer.targetY - pointer.y) * Math.min(1, dt * 5.5);
    applyParallax();

    if (width === 0 || height === 0) resize();
    ctx.clearRect(0, 0, width, height);
    drawScene(ctx, width, height, dt, elapsed, pointer);

    rafId = requestAnimationFrame(tick);
  }

  function destroy() {
    if (destroyed) return;
    destroyed = true;
    ac.abort();
    if (rafId) {
      cancelAnimationFrame(rafId);
      rafId = 0;
    }
    resizeObs?.disconnect();
    interObs?.disconnect();
    canvas.remove();
  }

  resize();
  rafId = requestAnimationFrame(tick);

  return { destroy };
}
