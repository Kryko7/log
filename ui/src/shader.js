// WebGL2 ambient kozo paper fiber & warm desk-lamp shader canvas.
// Degrades silently with zero console errors to the SVG feTurbulence backdrop
// when WebGL2 is unavailable.

const VERT_SRC = `#version 300 es
precision highp float;

const vec2 POS[3] = vec2[3](
  vec2(-1.0, -1.0),
  vec2( 3.0, -1.0),
  vec2(-1.0,  3.0)
);

out vec2 v_uv;

void main() {
  vec2 p = POS[gl_VertexID];
  gl_Position = vec4(p, 0.0, 1.0);
  v_uv = vec2((p.x + 1.0) * 0.5, 1.0 - (p.y + 1.0) * 0.5);
}
`;

const FRAG_SRC = `#version 300 es
precision highp float;

uniform vec2 u_resolution;
uniform vec2 u_cursor;
uniform float u_time;
uniform float u_night_mix;

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
  vec2 c_aspect = vec2(u_cursor.x * aspect, u_cursor.y);

  float macro_wash = fbm(p_aspect * 3.2);
  float fine_grain = noise2(v_uv * u_resolution * 0.42);
  float kozo_h = fbm(vec2(p_aspect.x * 18.0, p_aspect.y * 115.0));
  float kozo_d = fbm(vec2((p_aspect.x + p_aspect.y) * 58.0, (p_aspect.x - p_aspect.y) * 22.0));
  float fiber = (kozo_h * 0.55 + kozo_d * 0.45) - 0.5;

  vec2 centered = v_uv - vec2(0.5, 0.5);
  float vignette = smoothstep(0.92, 0.16, length(centered));

  float dist = distance(p_aspect, c_aspect);
  float lamp_breath = 1.0 + 0.018 * sin(u_time * 0.65);
  float day_sun = exp(-dist * dist * 2.2) * 0.045;
  float night_lamp = exp(-dist * dist * 2.8) * 0.24 * lamp_breath;

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

/**
 * Initialize the WebGL2 paper fiber & desk-lamp shader on a canvas element.
 * Gracefully degrades to the SVG feTurbulence backdrop if WebGL2 is unavailable.
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

  if (!canvasEl || typeof canvasEl.getContext !== "function") {
    return {
      setNightMode(isNight) {
        state.targetNight = isNight ? 1.0 : 0.0;
      },
      destroy() {},
    };
  }

  let gl = null;
  try {
    gl = canvasEl.getContext("webgl2", {
      alpha: false,
      antialias: false,
      depth: false,
      stencil: false,
      powerPreference: "low-power",
      preserveDrawingBuffer: false,
    });
  } catch {
    gl = null;
  }

  if (!gl) {
    return {
      setNightMode(isNight) {
        state.targetNight = isNight ? 1.0 : 0.0;
      },
      destroy() {},
    };
  }

  const vs = compileShader(gl, gl.VERTEX_SHADER, VERT_SRC);
  const fs = compileShader(gl, gl.FRAGMENT_SHADER, FRAG_SRC);
  if (!vs || !fs) {
    if (vs) gl.deleteShader(vs);
    if (fs) gl.deleteShader(fs);
    return {
      setNightMode(isNight) {
        state.targetNight = isNight ? 1.0 : 0.0;
      },
      destroy() {},
    };
  }

  const program = gl.createProgram();
  gl.attachShader(program, vs);
  gl.attachShader(program, fs);
  gl.linkProgram(program);
  gl.deleteShader(vs);
  gl.deleteShader(fs);

  if (!gl.getProgramParameter(program, gl.LINK_STATUS)) {
    gl.deleteProgram(program);
    return {
      setNightMode(isNight) {
        state.targetNight = isNight ? 1.0 : 0.0;
      },
      destroy() {},
    };
  }

  const vao = gl.createVertexArray();
  const locRes = gl.getUniformLocation(program, "u_resolution");
  const locCursor = gl.getUniformLocation(program, "u_cursor");
  const locTime = gl.getUniformLocation(program, "u_time");
  const locNight = gl.getUniformLocation(program, "u_night_mix");

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
    gl.viewport(0, 0, canvasEl.width, canvasEl.height);
  }

  function onMouseMove(ev) {
    const w = Math.max(window.innerWidth || 1280, 1);
    const h = Math.max(window.innerHeight || 720, 1);
    state.targetCursor[0] = Math.min(Math.max(ev.clientX / w, 0), 1);
    state.targetCursor[1] = Math.min(Math.max(ev.clientY / h, 0), 1);
  }

  syncSize();
  window.addEventListener("mousemove", onMouseMove, { passive: true });
  window.addEventListener("resize", syncSize, { passive: true });

  canvasEl.classList.add("is-active");
  const fallback = document.getElementById("paper-fallback");
  if (fallback) fallback.classList.add("is-gpu-ready");

  let rafId = 0;
  let destroyed = false;
  const startMs = performance.now();

  function frame(nowMs) {
    if (destroyed) return;
    state.smoothCursor[0] += (state.targetCursor[0] - state.smoothCursor[0]) * 0.08;
    state.smoothCursor[1] += (state.targetCursor[1] - state.smoothCursor[1]) * 0.08;
    state.smoothNight += (state.targetNight - state.smoothNight) * 0.09;

    gl.useProgram(program);
    gl.bindVertexArray(vao);
    gl.uniform2f(locRes, canvasEl.width, canvasEl.height);
    gl.uniform2f(locCursor, state.smoothCursor[0], state.smoothCursor[1]);
    gl.uniform1f(locTime, (nowMs - startMs) * 0.001);
    gl.uniform1f(locNight, state.smoothNight);
    gl.drawArrays(gl.TRIANGLES, 0, 3);

    rafId = window.requestAnimationFrame(frame);
  }

  rafId = window.requestAnimationFrame(frame);

  return {
    setNightMode(isNight) {
      state.targetNight = isNight ? 1.0 : 0.0;
    },
    destroy() {
      if (destroyed) return;
      destroyed = true;
      window.cancelAnimationFrame(rafId);
      window.removeEventListener("mousemove", onMouseMove);
      window.removeEventListener("resize", syncSize);
      gl.deleteVertexArray(vao);
      gl.deleteProgram(program);
    },
  };
}
