function mockEnvironment() {
  const make2d = () => {
    const state = {fillStyle: '', globalCompositeOperation: ''};
    const noop = () => {};
    return Object.assign(state, {
      fillRect: noop,
      beginPath: noop,
      arc: noop,
      rect: noop,
      closePath: noop,
      fill: noop,
      stroke: noop,
      fillText: noop,
      strokeText: noop,
      save: noop,
      restore: noop,
      translate: noop,
      rotate: noop,
      scale: noop,
      isPointInPath: () => false,
      measureText: () => ({width: 10}),
    });
  };
  const makeWebgl = () => {
    const params = {
      VERSION: 'WebGL 1.0',
      RENDERER: 'Mock Renderer',
      VENDOR: 'Mock Vendor',
      MAX_TEXTURE_SIZE: 8192,
      MAX_VIEWPORT_DIMS: [8192, 8192],
      MAX_VERTEX_ATTRIBS: 16,
      MAX_TEXTURE_IMAGE_UNITS: 16,
      MAX_VARYING_VECTORS: 30,
      MAX_COMBINED_TEXTURE_IMAGE_UNITS: 32,
      MAX_VERTEX_UNIFORM_VECTORS: 4096,
      MAX_FRAGMENT_UNIFORM_VECTORS: 1024,
      MAX_VERTEX_TEXTURE_IMAGE_UNITS: 16,
      MAX_CUBE_MAP_TEXTURE_SIZE: 16384,
      MAX_RENDERBUFFER_SIZE: 16384,
      RED_BITS: 8, GREEN_BITS: 8, BLUE_BITS: 8, ALPHA_BITS: 8,
      DEPTH_BITS: 24, STENCIL_BITS: 8,
      ALIASED_LINE_WIDTH_RANGE: [1, 1],
      ALIASED_POINT_SIZE_RANGE: [1, 1024],
      SHADING_LANGUAGE_VERSION: 'WebGL GLSL ES 1.0',
      VERTEX_SHADER: 1, FRAGMENT_SHADER: 2,
      LOW_FLOAT: 3, MEDIUM_FLOAT: 4, HIGH_FLOAT: 5,
      LOW_INT: 6, MEDIUM_INT: 7, HIGH_INT: 8,
      MAX_TEXTURE_MAX_ANISOTROPY_EXT: 16,
    };
    return {
      getParameter: (name) => (name in params ? params[name] : 0),
      getExtension: () => ({MAX_TEXTURE_MAX_ANISOTROPY_EXT: 16, UNMASKED_VENDOR_WEBGL: 1, UNMASKED_RENDERER_WEBGL: 2}),
      getSupportedExtensions: () => ['WEBGL_debug_renderer_info'],
      getShaderPrecisionFormat: () => ({rangeMin: 127, rangeMax: 127, precision: 23}),
      getContextAttributes: () => ({alpha: true, antialias: true, depth: true, stencil: false}),
      createBuffer() { return {}; },
      bindBuffer() {},
      bufferData() {},
      createProgram() { return {}; },
      createShader() { return {}; },
      shaderSource() {},
      compileShader() {},
      attachShader() {},
      linkProgram() {},
      useProgram() {},
      getAttribLocation() { return 0; },
      getUniformLocation() { return 0; },
      enableVertexAttribArray() {},
      vertexAttribPointer() {},
      drawArrays() {},
      clear() {},
      clearColor() {},
      clearDepth() {},
      depthFunc() {},
      enable() {},
      disable() {},
    };
  };
  const canvas = () => ({
    width: 200,
    height: 100,
    getContext(kind) {
      if (kind === '2d') return make2d();
      if (kind === 'webgl' || kind === 'experimental-webgl') return makeWebgl();
      return null;
    },
    toDataURL: () => 'data:image/png;base64,TU9DS0NBTlZBUw==',
  });
  const element = (tag) => {
    const node = {
      tagName: String(tag).toUpperCase(),
      style: {},
      children: [],
      offsetWidth: 72,
      offsetHeight: 18,
      innerHTML: '',
      textContent: '',
      innerText: '',
      appendChild(child) { node.children.push(child); return child; },
      removeChild(child) {
        const at = node.children.indexOf(child);
        if (at >= 0) node.children.splice(at, 1);
        return child;
      },
      setAttribute() {},
      addEventListener() {},
      removeEventListener() {},
      getElementsByTagName() { return []; },
      getContext(kind) { return canvas().getContext(kind); },
      toDataURL: () => 'data:image/png;base64,TU9DS0NBTlZBUw==',
    };
    if (String(tag).toLowerCase() === 'canvas') Object.assign(node, canvas());
    return node;
  };
  const plugins = [
    Object.assign([{type: 'application/pdf', suffixes: 'pdf'}], {
      name: 'PDF Viewer',
      description: 'Portable Document Format',
    }),
  ];
  const navigator = {
    userAgent:
      'Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36',
    appName: 'Netscape',
    appVersion: '5.0 (X11; Linux x86_64)',
    appCodeName: 'Mozilla',
    platform: 'Linux x86_64',
    product: 'Gecko',
    productSub: '20030107',
    vendor: '',
    language: 'zh-CN',
    languages: ['zh-CN', 'zh'],
    hardwareConcurrency: 8,
    deviceMemory: 8,
    cookieEnabled: true,
    doNotTrack: null,
    maxTouchPoints: 0,
    plugins,
    mimeTypes: [],
    javaEnabled: () => false,
    oscpu: '',
  };
  const document = {
    documentElement: Object.assign(element('html'), {clientWidth: 1280, clientHeight: 720}),
    body: Object.assign(element('body'), {clientWidth: 1280, clientHeight: 720}),
    title: 'Cumulus measurement harness',
    compatMode: 'CSS1Compat',
    documentMode: undefined,
    createElement: (tag) => element(tag),
    getElementsByTagName: (tag) => [element(tag)],
    cookie: '',
  };
  const window = {
    navigator,
    document,
    screen: {width: 1920, height: 1080, availWidth: 1920, availHeight: 1040, colorDepth: 24},
    devicePixelRatio: 1,
    innerWidth: 1280,
    innerHeight: 720,
    location: {href: 'https://music.163.com/', hostname: 'music.163.com', protocol: 'https:'},
    history: {length: 2},
    performance: {navigation: {type: 0}, timing: {responseStart: 10, requestStart: 5, responseEnd: 20, fetchStart: 0}},
    WebGLRenderingContext: function WebGLRenderingContext() {},
    XMLHttpRequest: function XMLHttpRequest() {},
    OfflineAudioContext: function OfflineAudioContext() {},
    localStorage: {getItem: () => null, setItem() {}, removeItem() {}},
    sessionStorage: {getItem: () => null, setItem() {}, removeItem() {}},
    setTimeout,
    clearTimeout,
    console,
    Math,
    Date,
    JSON,
    Object,
    Array,
    String,
    Number,
    Boolean,
    RegExp,
    Error,
    TypeError,
    parseFloat,
    parseInt,
    isNaN,
    encodeURI,
    encodeURIComponent,
    decodeURI,
    decodeURIComponent,
    escape,
    unescape,
    eval,
  };
  window.window = window;
  window.self = window;
  window.top = window;
  window.parent = window;
  return window;
}

// Behavioural conformance for the recovered measurement scripts.
//
// The scripts run inside the WebView, so their output depends on the browser. These
// tests drive both builds against one frozen mock browser and compare the records the
// Rust side consumes: the same keys, the same values, the same failure shape. Nothing
// here asserts a real WebView value, and nothing initializes the SDK.
const assert = require('node:assert/strict');
const crypto = require('node:crypto');
const fs = require('node:fs');
const test = require('node:test');
const vm = require('node:vm');

const BUILDS = ['602a5ad7', 'e2891084'];
const TIMING = [3, 0, 4, 1, 5, 2];
const ELAPSED = 7;
const FINGERPRINTS = [['Jb', 'plugins'], ['Qa', 'canvas'], ['dc', 'webgl'], ['jb', 'fonts']];
const ENVIRONMENT_KEYS = [
  'nb', 'hb', 'Eb', 'gb', 'Xb', 'wb',
  'sb', 'kb', 'vb', 'Ob', 'qb', 'Wb', 'yb', 'Tb', 'rb', 'ub', 'lb', 'Hb', 'Sa', 'Qb', 'cc',
];
const GLOBALS = [
  'console', 'Math', 'Date', 'JSON', 'Object', 'Array', 'String', 'Number', 'Boolean',
  'RegExp', 'Error', 'TypeError', 'Promise', 'Float32Array', 'Uint8Array', 'Uint16Array',
  'Int32Array', 'parseFloat', 'parseInt', 'isNaN', 'encodeURI', 'encodeURIComponent',
  'decodeURI', 'decodeURIComponent', 'escape', 'unescape',
];

function scriptPath(build, name) {
  return 'desktop/src/watchman/measurements/' + build + '/' + name + '.js';
}

// Evaluate one script as an expression inside a fresh mock browser.
function load(build, name) {
  const file = scriptPath(build, name);
  const source = fs.readFileSync(file, 'utf8');
  const window = mockEnvironment();
  const sandbox = Object.create(null);
  Object.assign(sandbox, {
    window,
    document: window.document,
    navigator: window.navigator,
    screen: window.screen,
    location: window.location,
    history: window.history,
    performance: window.performance,
    localStorage: window.localStorage,
    sessionStorage: window.sessionStorage,
    setTimeout,
    clearTimeout,
    XMLHttpRequest: window.XMLHttpRequest,
    WebGLRenderingContext: window.WebGLRenderingContext,
    OfflineAudioContext: window.OfflineAudioContext,
  });
  for (const name of GLOBALS) sandbox[name] = globalThis[name];
  sandbox.globalThis = sandbox;
  const context = vm.createContext(sandbox);
  const measure = vm.runInContext('(' + source + '\n)', context, {timeout: 10000});
  assert.equal(typeof measure, 'function', file + ' did not evaluate to a function');
  return {measure, window, source, file};
}

// Records are built inside a vm realm, so their objects carry that realm's prototypes
// and `deepStrictEqual` would reject two identical records. Compare plain JSON instead:
// that is also what crosses the IPC boundary.
function plain(value) {
  return JSON.parse(JSON.stringify(value));
}

function deviceRecord(build) {
  const {measure, window} = load(build, 'device');
  return measure({navigator: window.navigator}, undefined);
}

function environmentRecord(build) {
  const {measure, window} = load(build, 'environment');
  const environment = measure({navigator: window.navigator});
  const out = {};
  for (const key of ENVIRONMENT_KEYS) out[key] = environment.environment(key, TIMING.slice(), ELAPSED);
  out.fontCount = environment.fontCount();
  return out;
}

function fingerprintRecord(build, name) {
  const {measure, window} = load(build, name);
  return measure({navigator: window.navigator});
}

test('e2891084 scripts match the digests recorded by the generator', () => {
  const manifest = JSON.parse(fs.readFileSync('docs/watchman-measurements-min-e2891084.json', 'utf8'));
  const recorded = new Map(manifest.measurements.map((m) => [m.key, m]));
  for (const [, name] of FINGERPRINTS) {
    const key = name === 'plugins' ? 'Jb' : name === 'canvas' ? 'Qa' : name === 'webgl' ? 'dc' : 'jb';
    assert.equal(recorded.get(key).path, scriptPath('e2891084', name));
  }
  for (const [key, entry] of recorded) {
    const file = entry.key === 'device' ? scriptPath('e2891084', 'device')
      : entry.key === 'environment' ? scriptPath('e2891084', 'environment')
      : scriptPath('e2891084', {Jb: 'plugins', Qa: 'canvas', dc: 'webgl', jb: 'fonts'}[entry.key]);
    const actual = crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex');
    assert.equal(actual, entry.sha256, file + ' changed since the generator ran');
  }
});

test('device getters agree between builds and keep the official shape', () => {
  const rows = deviceRecord('e2891084');
  assert.equal(rows.length, 32);
  for (const row of rows) {
    assert.deepEqual(Object.keys(row).sort(), ['failed', 'key', 'value']);
    assert.equal(typeof row.key, 'string');
    assert.equal(typeof row.failed, 'boolean');
  }
  assert.deepEqual(plain(rows), plain(deviceRecord('602a5ad7')));
  // The four fingerprints, the font count and the audio callback have their own scripts
  // and must not also appear as plain device rows.
  for (const key of ['Jb', 'Qa', 'dc', 'jb', 'ib', 'La']) {
    assert.equal(rows.some((row) => row.key === key), false, key + ' belongs to another script');
  }
});

test('environment getters agree between builds for every official key', () => {
  const e289 = environmentRecord('e2891084');
  assert.deepEqual(Object.keys(e289), [...ENVIRONMENT_KEYS, 'fontCount']);
  for (const key of ENVIRONMENT_KEYS) {
    assert.equal(typeof e289[key].failed, 'boolean');
    assert.deepEqual(plain(e289[key].timing), TIMING, key + ' did not receive the frozen timing');
  }
  assert.deepEqual(plain(e289), plain(environmentRecord('602a5ad7')));
});

test('fingerprint preimages agree between builds', () => {
  for (const [key, name] of FINGERPRINTS) {
    const e289 = fingerprintRecord('e2891084', name);
    assert.deepEqual(plain(e289), plain(fingerprintRecord('602a5ad7', name)), key + ' differs between builds');
  }
  // Three measurements produce a preimage in this mock; fonts depends on a host global
  // the archive never defines, so both builds must report the same hard failure instead
  // of an invented value.
  for (const name of ['plugins', 'canvas', 'webgl']) {
    const record = fingerprintRecord('e2891084', name);
    assert.equal(record.kind, 'fingerprint', name + ' lost its preimage');
    assert.equal(typeof record.preimage, 'string');
    assert.notEqual(record.preimage, '');
  }
  assert.deepEqual(plain(fingerprintRecord('e2891084', 'fonts')), {kind: 'failed'});
});

test('repeated evaluation is stable and unknown keys are refused', () => {
  assert.deepEqual(plain(deviceRecord('e2891084')), plain(deviceRecord('e2891084')));
  assert.deepEqual(plain(environmentRecord('e2891084')), plain(environmentRecord('e2891084')));
  const {measure, window} = load('e2891084', 'environment');
  const environment = measure({navigator: window.navigator});
  assert.throws(() => environment.environment('Cookie', TIMING.slice(), ELAPSED), /unreviewed/);
});
