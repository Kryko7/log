// 2-pass cached-grain WebGL2 ambient kozo paper fiber & warm desk-lamp shader.
// Bakes static procedural paper texture once per resize into an offscreen RGBA8
// FBO, then composites cursor-tracked lighting at native display refresh rate
// (60Hz/120Hz/144Hz/165Hz/240Hz+) and idles at 0% GPU once settled.
// Degrades silently with zero console errors to lightweight CSS gradients when
// WebGL2 is unavailable.

const VERT_SRC = `#version 300 es
precision highp float;

const vec2 POS[3] = vec2[3](
  vec2(-1.0, -1.0),
  vec2( 3.0, -1.0),
  vec2(-1.0,  3.0)
);

out vec2 v_uv;
out vec2 v_tex;

void main() {
  vec2 p = POS[gl_VertexID];
  gl_Position = vec4(p, 0.0, 1.0);
  v_tex = (p + 1.0) * 0.5;
  v_uv = vec2(v_tex.x, 1.0 - v_tex.y);
}
`;

const GRAIN_FRAG_SRC = `#version 300 es
precision highp float;

uniform vec2 u_resolution;

in vec2 v_uv;
out vec4 fragColor;

float hash21(vec2 p) {
  vec2 q = fract(p * vec2(123.34, 456.21));
  q += dot(q, q + 45.32);
  return fract(q.x * q.y);
}

float noise2(vec2 p) {
  vec2 i = floor(p);
  vec2 f = fract(p);
  vec2 u = f * f * (3.0 - 2.0 * f);
  float a = hash21(i);
  float b = hash21(i + vec2(1.0, 0.0));
  float c = hash21(i + vec2(0.0, 1.0));
  float d = hash21(i + vec2(1.0, 1.0));
  return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

float fbm(vec2 p) {
  float v = 0.0;
  float amp = 0.5;
  vec2 pos = p;
  for (int i = 0; i < 4; i++) {
    v += amp * noise2(pos);
    pos = pos * 2.03 + vec2(17.1, 31.7);
    amp *= 0.5;
  }
  return v;
}

void main() {
  float aspect = max(u_resolution.x / max(u_resolution.y, 1.0), 0.5);
  vec2 p_aspect = vec2(v_uv.x * aspect, v_uv.y);

  float macro_wash = fbm(p_aspect * 3.2);
  float fine_grain = noise2(v_uv * u_resolution * 0.42);
  float kozo_h = fbm(vec2(p_aspect.x * 18.0, p_aspect.y * 115.0));
  float kozo_d = fbm(vec2((p_aspect.x + p_aspect.y) * 58.0, (p_aspect.x - p_aspect.y) * 22.0));
  float fiber = (kozo_h * 0.55 + kozo_d * 0.45) - 0.5;

  vec2 centered = v_uv - vec2(0.5, 0.5);
  float vignette = smoothstep(0.92, 0.16, length(centered));

  fragColor = vec4(
    clamp(macro_wash, 0.0, 1.0),
    clamp(fiber + 0.5, 0.0, 1.0),
    clamp(fine_grain, 0.0, 1.0),
    clamp(vignette, 0.0, 1.0)
  );
}
`;

const COMP_FRAG_SRC = `#version 300 es
precision highp float;

uniform sampler2D u_grain;
uniform vec2 u_resolution;
uniform vec2 u_cursor;
uniform float u_night_mix;

in vec2 v_uv;
in vec2 v_tex;
out vec4 fragColor;

void main() {
  vec4 g = texture(u_grain, v_tex);
  float macro_wash = g.r;
  float fiber = g.g - 0.5;
  float fine_grain = g.b;
  float vignette = g.a;

  float aspect = max(u_resolution.x / max(u_resolution.y, 1.0), 0.5);
  vec2 p_aspect = vec2(v_uv.x * aspect, v_uv.y);
  vec2 c_aspect = vec2(u_cursor.x * aspect, u_cursor.y);

  float dist = distance(p_aspect, c_aspect);
  float day_sun = exp(-dist * dist * 2.2) * 0.045;
  float night_lamp = exp(-dist * dist * 2.8) * 0.24;

  // Day Cream Paper (#EDE5D7 desk -> #FBF7F0 warm paper)
  vec3 day_base = vec3(0.935, 0.905, 0.852);
  vec3 day_warm = vec3(0.984, 0.969, 0.941);
  vec3 day_col = mix(day_base, day_warm, 0.35 + 0.45 * vignette + 0.2 * macro_wash);
  day_col += vec3(0.045, 0.038, 0.025) * day_sun;
  day_col += vec3(0.022, 0.019, 0.015) * fiber;
  day_col += vec3(0.010, 0.009, 0.008) * (fine_grain - 0.5);

  // Candlelit Night Desk (#141212 slate + #D99B52 warm amber desk lamp)
  vec3 night_base = vec3(0.075, 0.068, 0.068);
  vec3 night_elev = vec3(0.112, 0.098, 0.094);
  vec3 amber_glow = vec3(0.851, 0.608, 0.322);
  vec3 night_col = mix(night_base, night_elev, 0.35 + 0.55 * vignette + 0.15 * macro_wash);
  night_col = mix(night_col, amber_glow, clamp(night_lamp, 0.0, 0.22));
  night_col += vec3(0.018, 0.014, 0.010) * fiber * (0.6 + night_lamp * 2.0);
  night_col += vec3(0.007, 0.006, 0.006) * (fine_grain - 0.5);

  vec3 color = mix(day_col, night_col, clamp(u_night_mix, 0.0, 1.0));
  fragColor = vec4(color, 1.0);
}
`;

function compileShader(gl, type, source) {
  const shader = gl.createShader(type);
  if (!shader) return null;
  gl.shaderSource(shader, source);
  gl.compileShader(shader);
  if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) {
    gl.deleteShader(shader);
    return null;
  }
  return shader;
}

function createProgram(gl, vsSource, fsSource) {
  const vs = compileShader(gl, gl.VERTEX_SHADER, vsSource);
  const fs = compileShader(gl, gl.FRAGMENT_SHADER, fsSource);
  if (!vs || !fs) {
    if (vs) gl.deleteShader(vs);
    if (fs) gl.deleteShader(fs);
    return null;
  }
  const program = gl.createProgram();
  gl.attachShader(program, vs);
  gl.attachShader(program, fs);
  gl.linkProgram(program);
  gl.deleteShader(vs);
  gl.deleteShader(fs);
  if (!gl.getProgramParameter(program, gl.LINK_STATUS)) {
    gl.deleteProgram(program);
    return null;
  }
  return program;
}

/**
 * Initialize the 2-pass WebGL2 paper fiber & desk-lamp shader on a canvas element.
 * Gracefully degrades to lightweight CSS gradients if WebGL2 is unavailable.
 * @param {HTMLCanvasElement|null} canvasEl
 * @param {boolean} [initialNight=false]
 * @returns {{ setNightMode: (isNight: boolean) => void, destroy: () => void }}
 */
export function initPaperShader(canvasEl, initialNight = false) {
  const state = {
    targetNight: initialNight ? 1.0 : 0.0,
    smoothNight: initialNight ? 1.0 : 0.0,
    targetCursor: [0.52, 0.34],
    smoothCursor: [0.52, 0.34],
  };

  const noopHandle = {
    setNightMode(isNight) {
      state.targetNight = isNight ? 1.0 : 0.0;
    },
    destroy() {},
  };

  if (!canvasEl || typeof canvasEl.getContext !== "function") {
    return noopHandle;
  }

  let gl = null;
  try {
    gl = canvasEl.getContext("webgl2", {
      alpha: false,
      antialias: false,
      depth: false,
      stencil: false,
      powerPreference: "default",
      preserveDrawingBuffer: false,
    });
  } catch {
    gl = null;
  }

  if (!gl) {
    return noopHandle;
  }

  const grainProgram = createProgram(gl, VERT_SRC, GRAIN_FRAG_SRC);
  const compProgram = createProgram(gl, VERT_SRC, COMP_FRAG_SRC);
  if (!grainProgram || !compProgram) {
    if (grainProgram) gl.deleteProgram(grainProgram);
    if (compProgram) gl.deleteProgram(compProgram);
    return noopHandle;
  }

  const vao = gl.createVertexArray();
  const grainFbo = gl.createFramebuffer();
  const grainTex = gl.createTexture();

  const locGrainRes = gl.getUniformLocation(grainProgram, "u_resolution");
  const locCompGrain = gl.getUniformLocation(compProgram, "u_grain");
  const locCompRes = gl.getUniformLocation(compProgram, "u_resolution");
  const locCompCursor = gl.getUniformLocation(compProgram, "u_cursor");
  const locCompNight = gl.getUniformLocation(compProgram, "u_night_mix");

  let bakedWidth = 0;
  let bakedHeight = 0;

  function bakeGrain(cw, ch) {
    gl.bindTexture(gl.TEXTURE_2D, grainTex);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
    gl.texImage2D(
      gl.TEXTURE_2D,
      0,
      gl.RGBA8,
      cw,
      ch,
      0,
      gl.RGBA,
      gl.UNSIGNED_BYTE,
      null
    );

    gl.bindFramebuffer(gl.FRAMEBUFFER, grainFbo);
    gl.framebufferTexture2D(
      gl.FRAMEBUFFER,
      gl.COLOR_ATTACHMENT0,
      gl.TEXTURE_2D,
      grainTex,
      0
    );

    if (gl.checkFramebufferStatus(gl.FRAMEBUFFER) !== gl.FRAMEBUFFER_COMPLETE) {
      gl.bindFramebuffer(gl.FRAMEBUFFER, null);
      gl.bindTexture(gl.TEXTURE_2D, null);
      return false;
    }

    gl.viewport(0, 0, cw, ch);
    gl.useProgram(grainProgram);
    gl.bindVertexArray(vao);
    gl.uniform2f(locGrainRes, cw, ch);
    gl.drawArrays(gl.TRIANGLES, 0, 3);

    gl.bindFramebuffer(gl.FRAMEBUFFER, null);
    gl.bindTexture(gl.TEXTURE_2D, null);
    bakedWidth = cw;
    bakedHeight = ch;
    return true;
  }

  function syncSize() {
    const dpr = Math.min(Math.max(window.devicePixelRatio || 1, 1), 1.5);
    const w = Math.max(Math.round((window.innerWidth || 1280) * dpr), 320);
    const h = Math.max(Math.round((window.innerHeight || 720) * dpr), 240);
    const cw = Math.min(w, 2048);
    const ch = Math.min(h, 2048);
    if (canvasEl.width !== cw || canvasEl.height !== ch) {
      canvasEl.width = cw;
      canvasEl.height = ch;
    }
    if (bakedWidth !== cw || bakedHeight !== ch) {
      if (!bakeGrain(cw, ch)) {
        return false;
      }
    }
    gl.viewport(0, 0, canvasEl.width, canvasEl.height);
    return true;
  }

  const reducedQuery =
    typeof window.matchMedia === "function"
      ? window.matchMedia("(prefers-reduced-motion: reduce)")
      : null;
  let reducedMotion = Boolean(reducedQuery?.matches);
  let rafId = 0;
  let destroyed = false;
  let lastFrameMs = 0;

  function drawFrame() {
    gl.bindFramebuffer(gl.FRAMEBUFFER, null);
    gl.viewport(0, 0, canvasEl.width, canvasEl.height);
    gl.useProgram(compProgram);
    gl.bindVertexArray(vao);
    gl.activeTexture(gl.TEXTURE0);
    gl.bindTexture(gl.TEXTURE_2D, grainTex);
    gl.uniform1i(locCompGrain, 0);
    gl.uniform2f(locCompRes, canvasEl.width, canvasEl.height);
    gl.uniform2f(locCompCursor, state.smoothCursor[0], state.smoothCursor[1]);
    gl.uniform1f(locCompNight, state.smoothNight);
    gl.drawArrays(gl.TRIANGLES, 0, 3);
  }

  function needsAnimation() {
    if (reducedMotion) return false;
    const nightDelta = Math.abs(state.targetNight - state.smoothNight);
    const cursorDelta = Math.hypot(
      state.targetCursor[0] - state.smoothCursor[0],
      state.targetCursor[1] - state.smoothCursor[1]
    );
    return cursorDelta > 0.0004 || nightDelta > 0.0008;
  }

  function schedule() {
    if (destroyed || document.hidden || rafId !== 0 || !needsAnimation()) return;
    rafId = window.requestAnimationFrame(frame);
  }

  function frame(nowMs) {
    rafId = 0;
    if (destroyed || document.hidden) {
      lastFrameMs = 0;
      return;
    }

    const dtSec =
      lastFrameMs > 0
        ? Math.min(Math.max((nowMs - lastFrameMs) * 0.001, 0.0005), 0.05)
        : 1 / 60;
    lastFrameMs = nowMs;

    if (reducedMotion) {
      state.smoothCursor[0] = state.targetCursor[0];
      state.smoothCursor[1] = state.targetCursor[1];
      state.smoothNight = state.targetNight;
    } else {
      const alpha = 1 - Math.exp(-dtSec * 18);
      state.smoothCursor[0] += (state.targetCursor[0] - state.smoothCursor[0]) * alpha;
      state.smoothCursor[1] += (state.targetCursor[1] - state.smoothCursor[1]) * alpha;
      state.smoothNight += (state.targetNight - state.smoothNight) * alpha;

      if (Math.abs(state.targetNight - state.smoothNight) <= 0.0008) {
        state.smoothNight = state.targetNight;
      }
      if (
        Math.hypot(
          state.targetCursor[0] - state.smoothCursor[0],
          state.targetCursor[1] - state.smoothCursor[1]
        ) <= 0.0004
      ) {
        state.smoothCursor[0] = state.targetCursor[0];
        state.smoothCursor[1] = state.targetCursor[1];
      }
    }

    drawFrame();
    if (needsAnimation()) {
      schedule();
    } else {
      lastFrameMs = 0;
    }
  }

  function onPointerMove(ev) {
    const w = Math.max(window.innerWidth || 1280, 1);
    const h = Math.max(window.innerHeight || 720, 1);
    state.targetCursor[0] = Math.min(Math.max(ev.clientX / w, 0), 1);
    state.targetCursor[1] = Math.min(Math.max(ev.clientY / h, 0), 1);
    if (reducedMotion) {
      state.smoothCursor[0] = state.targetCursor[0];
      state.smoothCursor[1] = state.targetCursor[1];
      drawFrame();
    } else {
      schedule();
    }
  }

  function onResize() {
    if (!syncSize()) return;
    drawFrame();
    schedule();
  }

  function onVisibilityChange() {
    if (document.hidden) {
      if (rafId !== 0) {
        window.cancelAnimationFrame(rafId);
        rafId = 0;
      }
      lastFrameMs = 0;
    } else {
      drawFrame();
      schedule();
    }
  }

  function onReducedMotionChange(ev) {
    reducedMotion = Boolean(ev.matches);
    if (reducedMotion) {
      if (rafId !== 0) {
        window.cancelAnimationFrame(rafId);
        rafId = 0;
      }
      lastFrameMs = 0;
      state.smoothCursor[0] = state.targetCursor[0];
      state.smoothCursor[1] = state.targetCursor[1];
      state.smoothNight = state.targetNight;
      drawFrame();
    } else {
      schedule();
    }
  }

  if (!syncSize()) {
    gl.deleteTexture(grainTex);
    gl.deleteFramebuffer(grainFbo);
    gl.deleteVertexArray(vao);
    gl.deleteProgram(grainProgram);
    gl.deleteProgram(compProgram);
    return noopHandle;
  }

  window.addEventListener("pointermove", onPointerMove, { passive: true });
  window.addEventListener("resize", onResize, { passive: true });
  document.addEventListener("visibilitychange", onVisibilityChange);
  reducedQuery?.addEventListener?.("change", onReducedMotionChange);

  canvasEl.classList.add("is-active");
  const fallback = document.getElementById("paper-fallback");
  if (fallback) fallback.classList.add("is-gpu-ready");

  drawFrame();
  schedule();

  return {
    setNightMode(isNight) {
      state.targetNight = isNight ? 1.0 : 0.0;
      if (reducedMotion) {
        state.smoothNight = state.targetNight;
        drawFrame();
      } else {
        schedule();
      }
    },
    destroy() {
      if (destroyed) return;
      destroyed = true;
      if (rafId !== 0) {
        window.cancelAnimationFrame(rafId);
        rafId = 0;
      }
      window.removeEventListener("pointermove", onPointerMove);
      window.removeEventListener("resize", onResize);
      document.removeEventListener("visibilitychange", onVisibilityChange);
      reducedQuery?.removeEventListener?.("change", onReducedMotionChange);
      gl.deleteTexture(grainTex);
      gl.deleteFramebuffer(grainFbo);
      gl.deleteVertexArray(vao);
      gl.deleteProgram(grainProgram);
      gl.deleteProgram(compProgram);
    },
  };
}
