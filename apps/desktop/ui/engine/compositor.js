/** One WebGL2 compositor. Failure leaves the safe cover visible, never raw video. */
import { tuningParams } from './tuning.js';
import { LOOK } from './look.js';
const vertex = `#version 300 es
in vec2 position;
out vec2 uv;
void main() { uv = position * .5 + .5; gl_Position = vec4(position, 0., 1.); }`;
const common = `#version 300 es
precision highp float;
precision highp sampler2D;
in vec2 uv;
out vec4 color;
uniform sampler2D camera, source, matte, background, lookA, lookB;
uniform vec4 crop, imageCrop;
uniform bool mirror, cameraBitmap, backgroundBitmap, lookOn, lookSmooth;
uniform float lookMix, lookGain, lookWarmth;
uniform bool spotOn;
uniform vec4 spot, spotShape;   // spot: centre and reach in output pixels; shape: dim, lift, falloff
uniform vec2 outSize;
uniform float spotPresence;
vec2 cameraUV(vec2 v) {
  if (mirror) v.x = 1. - v.x;
  vec2 c = vec2(crop.x + v.x * crop.z, crop.y + (1. - v.y) * crop.w);
  return cameraBitmap ? c : vec2(c.x, 1. - c.y);
}
vec3 rawAt(vec2 v) { return textureLod(camera, cameraUV(v), 0.).rgb; }
float band(float x, float lo, float hi, float soft) {
  return smoothstep(lo - soft, lo + soft, x) * (1. - smoothstep(hi - soft, hi + soft, x));
}
float skinWeight(vec3 c) {
  float y = dot(c, vec3(.299, .587, .114)) * 255.;
  float cb = 128. + dot(c, vec3(-.168736, -.331264, .5)) * 255.;
  float cr = 128. + dot(c, vec3(.5, -.418688, -.081312)) * 255.;
  return band(cb, 77., 127., 6.) * band(cr, 133., 173., 6.) * band(y, 35., 250., 15.);
}
// Touch-ups, in the same order as crates/engine/src/appearance.rs: smoothing on skin, then
// the low-light curve, then warmth. lookA/lookB hold the guided filter (a, skin) and (b).
vec3 cameraAt(vec2 v) {
  vec3 c = rawAt(v);
  if (!lookOn) return c;
  if (lookSmooth) {
    vec4 a = texture(lookA, v);
    vec3 q = a.rgb * c + texture(lookB, v).rgb;
    c = mix(c, q, a.a * skinWeight(c) * lookMix);
  }
  c = clamp(c, 0., 1.);
  c = c * lookGain / (1. + (lookGain - 1.) * c);
  c *= vec3(1. + lookWarmth, 1., 1. - lookWarmth);
  c = clamp(c, 0., 1.);
  if (spotOn) {
    // Studio Light (studio_light.rs): lift the face, dim the room, fade with presence.
    vec2 d = (vec2(v.x, 1. - v.y) * outSize - spot.xy) / spot.zw;
    float k = 1. - smoothstep(spotShape.z, spotShape.w, length(d));
    vec3 dark = c * (1. - spotShape.x);
    vec3 bright = c * (1. + spotShape.y) / (1. + spotShape.y * c);
    c = mix(c, mix(dark, bright, k), spotPresence);
  }
  return clamp(c, 0., 1.);
}
float guide(vec2 v) { return dot(textureLod(camera, cameraUV(v), 1.).rgb, vec3(.2126,.7152,.0722)); }
vec3 linearize(vec3 v) { return pow(max(v, vec3(0.)), vec3(2.2)); }
vec3 encode(vec3 v) { return pow(max(v, vec3(0.)), vec3(1./2.2)); }
`;
const shaders = {
  // The touch-up's guided filter, at half size in output space.
  lookMoments: `uniform int which;
  void main() {
    vec3 c = texture(camera, cameraUV(uv)).rgb;
    color = which == 0 ? vec4(c, 1.) : vec4(c * c, 1.);
  }`,
  lookCoefficients: `uniform sampler2D squares;
  uniform float lookEpsilon;
  uniform int which;
  void main() {
    vec3 m = texture(source, uv).rgb, m2 = texture(squares, uv).rgb;
    vec3 a = max(m2 - m*m, vec3(0.)); a = a / (a + lookEpsilon);
    color = which == 0 ? vec4(a, skinWeight(m)) : vec4(m - a*m, 1.);
  }`,
  moments: `void main() {
    float i = guide(uv);
    vec2 c = cameraUV(uv);
    float p = texture(matte, vec2(c.x, cameraBitmap ? c.y : 1. - c.y)).r;
    color = vec4(i, p, i*i, i*p);
  }`,
  box: `uniform vec2 stepSize;
  void main() {
    color = vec4(0.);
    for (int n=-3; n<=3; n++) color += texture(source, uv + float(n)*stepSize);
    color /= 7.;
  }`,
  coefficients: `uniform float epsilon;
  void main() {
    vec4 m = texture(source, uv);
    float a = (m.w - m.x*m.y) / (max(0., m.z - m.x*m.x) + epsilon);
    color = vec4(a, m.y - a*m.x, 0., 1.);
  }`,
  alpha: `uniform bool hasMask;
  uniform float curveLow, curveHigh, curveMix;
  void main() {
    vec2 ab = texture(source, uv).rg;
    float a = hasMask ? clamp(ab.x*guide(uv) + ab.y, 0., 1.) : 0.;
    // Exactly one contrast curve, after the guided upsample.
    a = mix(a, smoothstep(curveLow, curveHigh, a), curveMix);
    color = vec4(a, a, a, 1.);
  }`,
  premultiply: `uniform bool hasMask;
  void main() {
    float weight = hasMask ? 1. - texture(matte, uv).r : 1.;
    color = vec4(linearize(cameraAt(uv)) * weight, weight);
  }`,
  kawase: `uniform vec2 stepSize;
  void main() {
    color = texture(source, uv) * 4.;
    color += texture(source, uv + stepSize*vec2(-1.,-1.));
    color += texture(source, uv + stepSize*vec2(1.,-1.));
    color += texture(source, uv + stepSize*vec2(-1.,1.));
    color += texture(source, uv + stepSize*vec2(1.,1.));
    color /= 8.;
  }`,
  composite: `uniform int mode;
  uniform float fade;
  uniform bool alphaOnly;
  void main() {
    if (mode == 0) { color = vec4(cameraAt(uv), 1.); return; }
    float a = texture(matte, uv).r * fade;
    if (alphaOnly) { color = vec4(vec3(a), 1.); return; }
    vec3 bg;
    if (mode == 2) {
      vec2 b = imageCrop.xy + uv*imageCrop.zw;
      if (backgroundBitmap) b.y = 1. - b.y;
      bg = linearize(texture(background, b).rgb);
    }
    else {
      vec4 b = texture(source, uv);
      // A very large person can leave no background samples. Neutral is private.
      bg = b.a > .002 ? b.rgb/b.a : vec3(.025);
    }
    vec3 fg = linearize(cameraAt(uv));
    color = vec4(encode(mix(bg, fg, a)), 1.);
  }`,
};
export function createCompositor({ onStatus = () => {} } = {}) {
  const canvas = document.createElement('canvas');
  const gl = canvas.getContext('webgl2', { alpha: false, antialias: false, depth: false });
  if (!gl) throw new Error('WebGL2 is required for background effects');
  let resources, lost = false, lastCamera, lastCameraKey, lastMask, lastBackground;
  function initialize() {
    if (!gl.getExtension('EXT_color_buffer_float')) throw new Error('Floating-point render targets unavailable');
    const buffer = gl.createBuffer(); gl.bindBuffer(gl.ARRAY_BUFFER, buffer);
    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1,-1, 1,-1, -1,1, 1,1]), gl.STATIC_DRAW);
    const programs = {};
    for (const [name, body] of Object.entries(shaders)) {
      const program = gl.createProgram();
      for (const [type, code] of [[gl.VERTEX_SHADER, vertex], [gl.FRAGMENT_SHADER, common + body]]) {
        const shader = gl.createShader(type); gl.shaderSource(shader, code); gl.compileShader(shader);
        if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) throw new Error(gl.getShaderInfoLog(shader));
        gl.attachShader(program, shader); gl.deleteShader(shader);
      }
      gl.linkProgram(program);
      if (!gl.getProgramParameter(program, gl.LINK_STATUS)) throw new Error(gl.getProgramInfoLog(program));
      const uniforms = {};
      for (let i=0; i<gl.getProgramParameter(program, gl.ACTIVE_UNIFORMS); i++) {
        const name = gl.getActiveUniform(program, i).name;
        uniforms[name] = gl.getUniformLocation(program, name);
      }
      programs[name] = { program, uniforms };
    }
    function texture() {
      const t = gl.createTexture(); gl.bindTexture(gl.TEXTURE_2D, t);
      for (const p of [gl.TEXTURE_MIN_FILTER, gl.TEXTURE_MAG_FILTER]) gl.texParameteri(gl.TEXTURE_2D, p, gl.LINEAR);
      for (const p of [gl.TEXTURE_WRAP_S, gl.TEXTURE_WRAP_T]) gl.texParameteri(gl.TEXTURE_2D, p, gl.CLAMP_TO_EDGE);
      gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, 1, 1, 0, gl.RGBA, gl.UNSIGNED_BYTE, new Uint8Array([0,0,0,255]));
      return t;
    }
    resources = { programs, buffer, framebuffer: gl.createFramebuffer(), camera: texture(), mask: texture(), background: texture(),
      targets: [], texture, width: 0, height: 0 };
    lastCamera = lastMask = lastBackground = null;
  }
  canvas.addEventListener('webglcontextlost', event => {
    event.preventDefault(); lost = true; onStatus('error', 'Graphics context lost; background remains covered.');
  });
  canvas.addEventListener('webglcontextrestored', () => {
    try { lost = false; initialize(); onStatus('restored', 'Graphics restored; rebuilding person mask.'); }
    catch (error) { lost = true; onStatus('error', error.message); }
  });
  initialize();
  function bind(unit, texture) {
    gl.activeTexture(gl.TEXTURE0 + unit); gl.bindTexture(gl.TEXTURE_2D, texture);
  }
  function resize(width, height) {
    if (resources.width === width && resources.height === height) return;
    for (const t of resources.targets) gl.deleteTexture(t.texture);
    resources.width = canvas.width = width; resources.height = canvas.height = height;
    const halfW = Math.ceil(width/2), halfH = Math.ceil(height/2);
    // Six guided-filter intermediates, alpha at output size, then a blur pyramid.
    const sizes = Array.from({ length: 6 }, () => [halfW, halfH]);
    sizes.push([width, height]);
    for (let level=1; level<=7; level++) sizes.push([Math.max(1, width>>level), Math.max(1, height>>level)]);
    // Four more at half size for the touch-up (14..17).
    for (let i=0; i<4; i++) sizes.push([halfW, halfH]);
    resources.targets = sizes.map(([w,h], index) => {
      const t = resources.texture(); bind(0, t);
      const floating = index !== 6;
      gl.texImage2D(gl.TEXTURE_2D, 0, floating ? gl.RGBA16F : gl.RGBA8, w, h, 0, gl.RGBA,
        floating ? gl.HALF_FLOAT : gl.UNSIGNED_BYTE, null);
      return { texture: t, width: w, height: h };
    });
  }
  function pass(name, target, inputs, params = {}) {
    const { program, uniforms: u } = resources.programs[name];
    gl.useProgram(program); gl.bindBuffer(gl.ARRAY_BUFFER, resources.buffer);
    const p = gl.getAttribLocation(program, 'position'); gl.enableVertexAttribArray(p); gl.vertexAttribPointer(p, 2, gl.FLOAT, false, 0, 0);
    let unit = 0;
    for (const [name, texture] of Object.entries(inputs)) { bind(unit, texture); gl.uniform1i(u[name] ?? null, unit++); }
    for (const [name, value] of Object.entries(params)) {
      const at = u[name]; if (at == null) continue;
      if (Array.isArray(value)) gl[`uniform${value.length}fv`](at, value);
      else if (['mirror','cameraBitmap','backgroundBitmap','hasMask','mode','alphaOnly','lookOn','lookSmooth','which','spotOn'].includes(name)) gl.uniform1i(at, value);
      else gl.uniform1f(at, value);
    }
    gl.bindFramebuffer(gl.FRAMEBUFFER, target ? resources.framebuffer : null);
    if (target) gl.framebufferTexture2D(gl.FRAMEBUFFER, gl.COLOR_ATTACHMENT0, gl.TEXTURE_2D, target.texture, 0);
    gl.viewport(0, 0, target?.width || canvas.width, target?.height || canvas.height);
    gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);
  }
  function uploadDOM(texture, image, mip = false) {
    bind(0, texture);
    gl.pixelStorei(gl.UNPACK_FLIP_Y_WEBGL, !(typeof ImageBitmap !== 'undefined' && image instanceof ImageBitmap));
    gl.pixelStorei(gl.UNPACK_PREMULTIPLY_ALPHA_WEBGL, false);
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, image);
    if (mip) {
      gl.generateMipmap(gl.TEXTURE_2D);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR_MIPMAP_LINEAR);
    }
  }
  return {
    canvas,
    draw({ image, frameKey, mask, crop = [0,0,1,1], mirror = false, background = {mode:'none'}, width, height, fade = 1, alphaOnly = false, tuning, look = null }) {
      if (lost || gl.isContextLost()) throw new Error('Graphics context unavailable');
      resize(width, height);
      const r = resources, targets = r.targets;
      const t = tuningParams(tuning), tap = t.radius / 6;
      if (image !== lastCamera || frameKey !== lastCameraKey) {
        uploadDOM(r.camera, image, true); lastCamera = image; lastCameraKey = frameKey;
      }
      if (mask && mask !== lastMask) {
        bind(0, r.mask);
        gl.pixelStorei(gl.UNPACK_FLIP_Y_WEBGL, false);
        gl.pixelStorei(gl.UNPACK_ALIGNMENT, 1);
        gl.texImage2D(gl.TEXTURE_2D, 0, gl.R8, mask.width, mask.height, 0, gl.RED, gl.UNSIGNED_BYTE, mask.alpha);
        lastMask = mask;
      }
      if (background.image && background.image !== lastBackground) {
        uploadDOM(r.background, background.image); lastBackground = background.image;
      }
      const params = { crop, mirror, cameraBitmap:typeof ImageBitmap !== 'undefined' && image instanceof ImageBitmap, hasMask: !!mask, curveLow:t.curveLow, curveHigh:t.curveHigh, curveMix:t.curveMix,
        lookOn: !!look && !alphaOnly, lookSmooth: !!look?.smooth, lookMix: look?.mix ?? 0, lookGain: look?.gain ?? 1, lookWarmth: look?.warmth ?? 0,
        spotOn: !!look?.spot && !alphaOnly, spot: look?.spot ? [look.spot.cx, look.spot.cy, Math.max(1, look.spot.rx), Math.max(1, look.spot.ry)] : [0,0,1,1],
        spotShape: look?.spotShape || [0,0,1,2], spotPresence: look?.spotPresence ?? 0, outSize: [width, height] };
      const lookInputs = {};
      if (params.lookOn && params.lookSmooth) {
        // Guided filter of the picture with itself (see appearance.rs): box means of I and
        // I², then a = var/(var+ε) and b = mean − a·mean, then box means of a and b.
        const [m, m2, a, b] = [targets[14], targets[15], targets[16], targets[17]];
        const radius = Math.max(1, Math.round(height * LOOK.smoothRadius));
        const step = [radius / 3 / m.width, 0], down = [0, radius / 3 / m.height];
        pass('lookMoments', m, { camera:r.camera }, { ...params, which:0 });
        pass('lookMoments', m2, { camera:r.camera }, { ...params, which:1 });
        pass('box', a, { source:m.texture }, { stepSize:step }); pass('box', m, { source:a.texture }, { stepSize:down });
        pass('box', a, { source:m2.texture }, { stepSize:step }); pass('box', m2, { source:a.texture }, { stepSize:down });
        pass('lookCoefficients', a, { source:m.texture, squares:m2.texture }, { lookEpsilon:look.epsilon, which:0 });
        pass('lookCoefficients', b, { source:m.texture, squares:m2.texture }, { lookEpsilon:look.epsilon, which:1 });
        pass('box', m, { source:a.texture }, { stepSize:step }); pass('box', a, { source:m.texture }, { stepSize:down });
        pass('box', m, { source:b.texture }, { stepSize:step }); pass('box', b, { source:m.texture }, { stepSize:down });
        lookInputs.lookA = a.texture; lookInputs.lookB = b.texture;
      }
      if (mask) {
        pass('moments', targets[0], { camera:r.camera, matte:r.mask }, params);
        pass('box', targets[1], { source:targets[0].texture }, { stepSize:[tap/targets[0].width,0] });
        pass('box', targets[2], { source:targets[1].texture }, { stepSize:[0,tap/targets[0].height] });
        pass('coefficients', targets[3], { source:targets[2].texture }, { epsilon:t.epsilon });
        pass('box', targets[4], { source:targets[3].texture }, { stepSize:[tap/targets[0].width,0] });
        pass('box', targets[5], { source:targets[4].texture }, { stepSize:[0,tap/targets[0].height] });
      }
      pass('alpha', targets[6], { camera:r.camera, source:targets[5].texture }, params);
      const mode = background.mode === 'none' ? 0 : background.mode === 'blur' ? 1 : 2;
      let blurred = targets[7];
      if (mode === 1) {
        pass('premultiply', targets[7], { camera:r.camera, matte:targets[6].texture, ...lookInputs }, params);
        // Small taps at successively lower resolution avoid separated ghost copies.
        const radius = mask ? Math.max(4, background.radius || 8) : 50;
        const levels = Math.min(6, Math.max(2, Math.ceil(Math.log2(radius))));
        for (let level=1; level<=levels; level++) {
          const target = targets[7+level];
          pass('kawase', target, { source:blurred.texture }, { stepSize:[1/blurred.width,1/blurred.height] });
          blurred = target;
        }
        for (let level=levels-1; level>=0; level--) {
          const target = targets[7+level];
          pass('kawase', target, { source:blurred.texture }, { stepSize:[.5/blurred.width,.5/blurred.height] });
          blurred = target;
        }
      }
      const iw = background.image?.naturalWidth || background.image?.width || 1;
      const ih = background.image?.naturalHeight || background.image?.height || 1;
      const ar = width/height, imageAr = iw/ih;
      const cw = Math.min(1, ar/imageAr), ch = Math.min(1, imageAr/ar);
      pass('composite', null, { camera:r.camera, matte:targets[6].texture, source:blurred.texture, background:r.background, ...lookInputs },
        { ...params, mode, fade, alphaOnly, backgroundBitmap:typeof ImageBitmap !== 'undefined' && background.image instanceof ImageBitmap, imageCrop:[(1-cw)/2,(1-ch)/2,cw,ch] });
      return canvas;
    },
    loseContext() { gl.getExtension('WEBGL_lose_context')?.loseContext(); },
    restoreContext() { gl.getExtension('WEBGL_lose_context')?.restoreContext(); },
    dispose() {
      if (!lost) {
        for (const p of Object.values(resources.programs)) gl.deleteProgram(p.program);
        for (const t of resources.targets) gl.deleteTexture(t.texture);
        for (const t of [resources.camera, resources.mask, resources.background]) gl.deleteTexture(t);
        gl.deleteFramebuffer(resources.framebuffer); gl.deleteBuffer(resources.buffer);
      }
      canvas.width = canvas.height = 1;
    },
  };
}
