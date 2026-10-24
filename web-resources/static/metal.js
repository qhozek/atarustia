// Liquid graphite: domain warped fbm (Inigo Quilez) read as a reflection map,
// bands of light run along the folds. Hash: Dave Hoskins, «Hash without Sine» (MIT).

const FRAGMENT = `
#ifdef GL_FRAGMENT_PRECISION_HIGH
precision highp float;
#else
precision mediump float;
#endif
uniform vec2 res;
uniform vec2 light;
uniform float time;
uniform float flash;

float hash(vec2 p) {
  vec3 q = fract(p.xyx * .1031);
  q += dot(q, q.yzx + 33.33);
  return fract((q.x + q.y) * q.z);
}

float noise(vec2 p) {
  vec2 i = floor(p), f = fract(p);
  f = f * f * f * (f * (f * 6. - 15.) + 10.);
  return mix(mix(hash(i), hash(i + vec2(1, 0)), f.x), mix(hash(i + vec2(0, 1)), hash(i + 1.), f.x), f.y);
}

float fbm(vec2 p) {
  float v = 0., a = .5;
  for (int i = 0; i < 4; i++) {
    v += a * noise(p);
    p = mat2(1.6, 1.2, -1.2, 1.6) * p;
    a *= .5;
  }
  return v;
}

void main() {
  vec2 uv = gl_FragCoord.xy / res;
  vec2 p = (gl_FragCoord.xy - .5 * res) / min(res.x, res.y) * 1.25;
  float t = time * .045;

  vec2 q = vec2(fbm(p + vec2(0, t)), fbm(p + vec2(5.2, 1.3) - vec2(t, 0)));
  vec2 r = vec2(fbm(p + 3. * q + vec2(1.7, 9.2) + .6 * t), fbm(p + 3. * q + vec2(8.3, 2.8) - .5 * t));
  float f = fbm(p + 2.6 * r);

  float phase = f * 2.4 + dot(uv, vec2(.6, .38)) * .85;
  float band = .5 + .5 * cos(6.2831853 * phase + time * .07);
  float v = mix(smoothstep(.22, .78, f), smoothstep(.04, .96, band), .72);
  v = v * .86 + pow(band, 10.) * smoothstep(.3, .7, r.x) * .22;
  v *= .62 + .55 * (1. - distance(uv, light) * .8);
  v += flash * (.35 * band + .15);

  vec3 c = mix(vec3(.015, .015, .017), vec3(.075, .075, .08), smoothstep(0., .42, v));
  c = mix(c, vec3(.19, .19, .2), smoothstep(.32, .78, v));
  c = mix(c, vec3(.52, .52, .55), smoothstep(.74, 1.12, v));
  c += (hash(gl_FragCoord.xy + fract(time * 3.3) * 97.) - .5) * .018;
  gl_FragColor = vec4(c, 1);
}`;

const still = matchMedia("(prefers-reduced-motion: reduce)");

/** Runs the shader on `canvas`; without WebGL the CSS background stays. */
export function metal(canvas) {
  const gl = canvas.getContext("webgl", { antialias: false, depth: false, stencil: false });
  const program = gl && gl.createProgram();
  const none = { flash() {}, aim() {} };
  if (!program) return none;

  for (const [type, source] of [
    [gl.VERTEX_SHADER, "attribute vec2 a;void main(){gl_Position=vec4(a,0,1);}"],
    [gl.FRAGMENT_SHADER, FRAGMENT],
  ]) {
    const shader = gl.createShader(type);
    gl.shaderSource(shader, source);
    gl.compileShader(shader);
    gl.attachShader(program, shader);
  }
  gl.bindAttribLocation(program, 0, "a");
  gl.linkProgram(program);
  if (!gl.getProgramParameter(program, gl.LINK_STATUS)) return none;

  gl.useProgram(program);
  gl.bindBuffer(gl.ARRAY_BUFFER, gl.createBuffer());
  gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 3, -1, -1, 3]), gl.STATIC_DRAW);
  gl.enableVertexAttribArray(0);
  gl.vertexAttribPointer(0, 2, gl.FLOAT, false, 0, 0);
  const u = Object.fromEntries(["res", "light", "time", "flash"].map((n) => [n, gl.getUniformLocation(program, n)]));

  let time = Math.random() * 40, flash = 0, speed = 1, raf = 0, last = 0;
  const light = [0.2, 0.85], target = [0.2, 0.85];

  function draw() {
    gl.viewport(0, 0, canvas.width, canvas.height);
    gl.uniform2f(u.res, canvas.width, canvas.height);
    gl.uniform2f(u.light, light[0], light[1]);
    gl.uniform1f(u.time, time);
    gl.uniform1f(u.flash, flash);
    gl.drawArrays(gl.TRIANGLES, 0, 3);
  }

  function frame(now) {
    raf = requestAnimationFrame(frame);
    const dt = Math.min(now - (last || now), 50) / 1000;
    last = now;
    speed += (1 - speed) * Math.min(1, dt * 1.2);
    time += dt * speed;
    flash *= Math.exp(-dt * 2.6);
    light[0] += (target[0] - light[0]) * Math.min(1, dt * 4);
    light[1] += (target[1] - light[1]) * Math.min(1, dt * 4);
    draw();
  }

  function sync() {
    const run = !document.hidden && !still.matches;
    if (run && !raf) {
      last = 0;
      raf = requestAnimationFrame(frame);
    } else if (!run && raf) {
      cancelAnimationFrame(raf);
      raf = 0;
    }
  }

  new ResizeObserver(() => {
    const dpr = Math.min(devicePixelRatio || 1, 1.5);
    canvas.width = Math.max(1, Math.round(canvas.clientWidth * dpr));
    canvas.height = Math.max(1, Math.round(canvas.clientHeight * dpr));
    draw();
    canvas.classList.add("ready");
  }).observe(canvas);
  document.addEventListener("visibilitychange", sync);
  still.addEventListener?.("change", sync);
  sync();

  return {
    /** Lightning over the surface, the folds speed up for a moment */
    flash() {
      if (!raf) return;
      flash = 1;
      speed = 9;
    },
    /** Moves the light to (x, y) in 0…1 of the canvas, y from the top */
    aim(x, y) {
      target[0] = x;
      target[1] = 1 - y;
    },
  };
}
