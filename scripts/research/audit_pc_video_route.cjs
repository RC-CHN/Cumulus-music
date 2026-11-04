'use strict';

// Offline replay of the exact recovered 3.1.42.205594 PC card and route
// expressions. Nothing from references is copied into the client or CI.
const assert = require('node:assert/strict');
const { createHash } = require('node:crypto');
const { readFileSync } = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const modules = process.argv[2];
if (!modules || process.argv.length !== 3) {
  process.stderr.write('usage: node scripts/research/audit_pc_video_route.cjs RECOVERED_JS_MODULES\n');
  process.exit(2);
}

const sourceNames = {
  routes: 'hybrid_app.chunk.1482700000000.js/205.js',
  api: 'hybrid_app.chunk.1482700000000.js/15.js',
  normalizer: 'hybrid_app.chunk.1482700000000.js/21.js',
  card: 'hybrid_41.chunk.1482700000000.js/2093.js',
  artistCard: 'hybrid_38.chunk.1482700000000.js/2093.js',
  artistView: 'hybrid_38.chunk.1482700000000.js/3088.js',
  action: 'hybrid_73.chunk.1482700000000.js/3085.js',
  view: 'hybrid_49.chunk.1482700000000.js/3098.js',
  model: 'hybrid_153.chunk.1482700000000.js/3158.js',
};
const sources = Object.fromEntries(
  Object.entries(sourceNames).map(([name, file]) => [name, readFileSync(path.join(modules, file), 'utf8')]),
);
const hashes = Object.fromEntries(
  Object.entries(sources).map(([name, contents]) => [name, createHash('sha256').update(contents).digest('hex')]),
);
assert.deepEqual(hashes, {
  routes: '80b8e0ac1ecb1df46a325c9acad261e896c3dc58109f6333f9cac00f49c62f49',
  api: '8ec4bf8d343f6963860eb73bc59ca09f0dd1ae50db2608a6c5abc7d69a0b521a',
  normalizer: '33d27af5a54c6a74641f192841637d2608d588d2d93091a9a61cb43948c93af7',
  card: '38aeb9fb0f4c68ff70080df44791f2180b75d67b974f38745d3780a7c0a701de',
  artistCard: '53c4e38fc270ab41ef89af4347524d0e538d76ed247297622bd263aac1cb5e28',
  artistView: '73601768f5fd234d511eee916bfa417ab30d875bd474e37b86a911d1c789976b',
  action: '121b1cc930b106acf5bb01d86305725ddd6abe2484120fee339a682301c7f0be',
  view: '3c36eca417d1ef901a205492c3c966f138ca2098417d449a21c06e175b7f3374',
  model: '8e88bb5d07c48c7ec485bf54784cf085824cb9515f026e9440fc1bac38bca4e4',
}, 'audit only executes the identified recovered source files');

function unique(source, marker, from = 0) {
  const start = source.indexOf(marker, from);
  assert(start !== -1, `missing source marker ${marker}`);
  assert.equal(source.indexOf(marker, start + marker.length), -1, `ambiguous marker ${marker}`);
  return start;
}

function section(source, startMarker, endMarker) {
  const start = unique(source, startMarker);
  return source.slice(start, unique(source, endMarker, start + startMarker.length));
}

function run(source, variables, filename) {
  const context = vm.createContext(variables, { codeGeneration: { strings: false, wasm: false } });
  return vm.runInContext(source, context, { filename, timeout: 1000 });
}

const local = (value) => JSON.parse(JSON.stringify(value));

async function replay() {
  const routeMap = section(sources.routes, '    videoDetail: {', '    switchAccount: {');
  assert.match(routeMap, /subPath: "\/:id\/:type\?"/);
  assert.match(routeMap, /bind\(null, 3098\)/);
  assert.match(routeMap, /bind\(null, 3158\)/);
  assert.match(sources.api, /"Qd", function \(\) \{\s+return Gt;/);
  assert.match(sources.api, /"Bg", function \(\) \{\s+return qt;/);
  assert.match(sources.normalizer, /return e\.video = "video", e\.mv = "mv", e;/);
  assert.match(sources.model, /await Object\(o\.Qd\)\(t\)/);
  assert.match(sources.model, /await Object\(o\.Bg\)\(t\)/);

  const rawOrdinaryId = 'AB1234567890CD1234567890EF123456';
  const rawMvId = '9007199254740993';
  const ordinaryRaw = {
    id: rawOrdinaryId,
    isMv: false,
    resource: {
      mlogBaseData: { type: 2, text: 'Synthetic video', duration: 5000 },
      mlogExtVO: { playCount: 1 },
      // There is no recovered contract to treat these fields as permission
      // or an authorized playback source; assert they never project.
      url: 'https://fixture.invalid/FALSE_AUTHORIZATION',
      videoUrl: 'https://fixture.invalid/FALSE_AUTHORIZATION',
    },
  };
  const dbWrites = [];
  const normalizer = section(sources.normalizer, '    V = async function () {', '    H = e =>');
  const normalize = run(
    `(() => { const ${normalizer.trim().replace(/,$/, '')}; return V; })()`,
    {
      L: { video: 'video', mv: 'mv' },
      I: async (values) => values ?? [],
      N: undefined,
      D: undefined,
      _: Object.assign,
      r: { Database: { put: (value) => dbWrites.push(value.tableName) } },
    },
    sourceNames.normalizer,
  );
  const [ordinary] = await normalize(ordinaryRaw);
  const [mv] = await normalize({
    id: rawMvId,
    resource: { mlogBaseData: { type: 3, text: 'Synthetic MV' } },
  });
  assert.equal(ordinary.type, 'video');
  assert.equal(ordinary.id, rawOrdinaryId);
  assert.equal(mv.type, 'mv');
  assert.equal(mv.id, rawMvId);
  const [decimalOrdinary] = await normalize({ ...ordinaryRaw, id: rawMvId });
  assert.equal(decimalOrdinary.id, mv.id);
  assert.equal(decimalOrdinary.type, 'video');
  assert(!JSON.stringify(ordinary).includes('FALSE_AUTHORIZATION'));

  const cardData = section(
    sources.card,
    '    const j = Object(n.useContext)(x.a),',
    '      R = Object(n.useMemo)',
  );
  const card = run(
    `((t) => { const l = "", m = null; ${cardData.trim().replace(/,$/, '')}; return U; })`,
    {
      n: { useContext: () => ({}) },
      x: { a: null },
      a: { b: { mv: 'mv' } },
      k: { a: { mv: 'mv', video: 'video' } },
      P: { a: { search: 'search' } },
      f: { a: { videoDetail: '/videoDetail' } },
    },
    sourceNames.card,
  );
  const explicitCard = card(ordinary);
  assert.equal(explicitCard.resourceType, 'video');
  assert.equal(explicitCard.to.linkPath, `/videoDetail/${rawOrdinaryId}`);
  assert.equal(card(mv).resourceType, 'mv');
  assert.equal(card(decimalOrdinary).resourceType, 'video');
  assert.match(sources.artistView, /mt = n\(2093\);\s+const _t = e => Object\(j.jsx\)\(mt.a, \{\s+video: e,/);
  const artistCardData = section(
    sources.artistCard,
    '    const v = Object(a.useContext)(P.a),',
    '      B = Object(a.useMemo)',
  );
  const artistCard = run(
    `((t) => { const o = "", O = "artist"; ${artistCardData.trim().replace(/,$/, '')}; return R; })`,
    {
      a: { useContext: () => ({}) },
      P: { a: null },
      r: { b: { mv: 'mv' } },
      k: { a: { mv: 'mv', video: 'video' } },
      T: { a: { search: 'search' } },
      f: { a: { videoDetail: '/videoDetail' } },
    },
    sourceNames.artistCard,
  );
  assert.equal(artistCard(ordinary).resourceType, 'video');
  assert.equal(artistCard(ordinary).to.linkPath, explicitCard.to.linkPath);
  const fallback = section(
    sources.action,
    '          case N.a.video:\n          case N.a.mv:',
    '          case N.a.algPlaylist:',
  );
  const genericLink = run(
    `((l, r) => { let y; switch (l) { ${fallback} } return y; })`,
    { N: { a: { mv: 'mv', video: 'video' } }, ye: { a: { videoDetail: '/videoDetail' } } },
    sourceNames.action,
  );
  assert.equal(genericLink('video', ordinary), explicitCard.to.linkPath);

  const view = section(sources.view, '    })(e => {\n      const {\n          dispatch: t,\n          data: n,', '    }),\n    $e = n(276);');
  function render({ id, type }, resourceType) {
    const requests = [];
    const viewNodes = [];
    const jsx = (node, props) => {
      if (typeof node === 'symbol') viewNodes.push(node.description);
      return props;
    };
    const component = run(`(${view.trimStart().slice('})('.length).trimEnd()}})`, {
      i: { router: { useParams: () => ({ id, type }) } },
      o: {
        useState: (value) => [value, () => {}],
        useRef: (value) => ({ current: value }),
        useCallback: (callback) => callback,
        useEffect: (effect) => effect(),
        useMemo: (compute) => compute(),
      },
      Ue: () => ({}),
      I: { a: () => ({}) },
      O: { jsx, jsxs: jsx },
      De: Symbol('mv-player'),
      Je: Symbol('info'),
      We: Symbol('error'),
      He: { a: Symbol('comments-wrapper') },
      a: { a: Symbol('comments') },
    }, sourceNames.view);
    component({
      data: null,
      uid: 'synthetic-user',
      isOnline: true,
      resourceType,
      dispatch(action) {
        requests.push(local(action));
        return Promise.resolve({ code: 403 });
      },
    });
    return { requests, mvPlayerMounted: viewNodes.includes('mv-player') };
  }
  const defaultOrdinary = render({ id: ordinary.id }, 'video');
  const explicitVideo = render({ id: ordinary.id, type: 'video' }, 'video');
  const explicitTwo = render({ id: ordinary.id, type: '2' }, 'video');
  const defaultMv = render({ id: mv.id }, 'mv');
  const detailAction = (id) => ({
    type: 'page:videoDetail/getDetail',
    payload: { id, type: '2' },
  });
  assert.deepEqual(defaultOrdinary, { requests: [detailAction(ordinary.id)], mvPlayerMounted: true });
  assert.deepEqual(defaultMv, { requests: [detailAction(mv.id)], mvPlayerMounted: true });
  assert.deepEqual(explicitVideo, { requests: [], mvPlayerMounted: false });
  assert.deepEqual(explicitTwo, { requests: [], mvPlayerMounted: false });

  const network = [];
  const apiBoundary = section(sources.api, '    Gt = Object(o.a)({', '    Wt = Object(o.a)(');
  const { Gt, qt } = run(
    `(() => { const ${apiBoundary.trim().replace(/,$/, '')}; return { Gt, qt }; })()`,
    {
      o: {
        a: (definition) => async (body) => {
          const endpoint = typeof definition === 'string' ? definition : definition.url;
          network.push({ endpoint, body: local(body) });
          if (endpoint === '/api/mlog/detail/v1') {
            return definition.format({ code: 200, data: ordinaryRaw });
          }
          assert.equal(endpoint, '/api/song/enhance/play/mv/url');
          return { code: 403 };
        },
      },
      r: { A: normalize },
      y: Object.assign,
      g: { enable: false },
    },
    sourceNames.api,
  );
  const requireStub = (name) => {
    if (name === 15) return { Qd: Gt, Bg: qt };
    if (name === 127) return { b: async () => null };
    if (name === 138) return { f: () => true }; // Synthetic no local MV takedown branch.
    assert([2, 3, 86, 157, 2112].includes(name), `unrecognized module ${name}`);
    return {};
  };
  requireStub.r = () => {};
  requireStub.d = (exports, name, read) => Object.defineProperty(exports, name, { get: read });
  const factory = run(sources.model, {}, sourceNames.model);
  const exports = {};
  factory({}, exports, requireStub);
  const model = await exports.default();
  const effects = {
    call: (fn, input) => fn(input),
    put: () => {},
    select: (read) => read({ download: { mvMap: new Map() } }),
  };
  async function drive(generator) {
    let step = generator.next();
    while (!step.done) {
      let value;
      try {
        value = await step.value;
      } catch (error) {
        step = generator.throw(error);
        continue;
      }
      step = generator.next(value);
    }
    return local(step.value);
  }
  const forced = await drive(model.effects.getDetail(detailAction(ordinary.id), effects));
  assert.equal(forced.data.type, 'mv');
  assert.equal(forced.data.id, ordinary.id);
  const failed = await drive(model.effects.getPlayUrl({ payload: { id: ordinary.id, r: 240 } }, effects));
  assert.equal(failed.code, 403);
  assert.deepEqual(network, [
    { endpoint: '/api/mlog/detail/v1', body: { id: ordinary.id, type: '2' } },
    { endpoint: '/api/song/enhance/play/mv/url', body: { id: ordinary.id, r: 240 } },
  ]);
  assert.equal(dbWrites.length, 4);
  return {
    ok: true,
    sourceHashes: Object.fromEntries(Object.entries(hashes).map(([name, hash]) => [sourceNames[name], hash])),
    classification: { ordinary: ordinary.type, mv: mv.type, opaqueIdPreserved: true, decimalIdDoesNotMakeMv: true },
    ordinaryCard: { resourceType: explicitCard.resourceType, explicitTypeSegment: false, artistCardAgrees: true },
    route: {
      defaultOrdinary: { detail: true, mvPlayer: true },
      explicitVideo: { detail: false, mvPlayer: false },
      explicitTwo: { detail: false, mvPlayer: false },
      defaultMv: { detail: true, mvPlayer: true },
    },
    recordedFixturePaths: network.map(({ endpoint }) => endpoint),
    ordinaryPayloadForcedIntoMvNormalization: true,
    syntheticMvAuthorizationResponse: 403,
    ordinaryAuthorizationContractRecovered: false,
    realRequests: 0,
  };
}

replay().then((result) => process.stdout.write(`${JSON.stringify(result, null, 2)}\n`))
  .catch((error) => { process.stderr.write(`${error.stack}\n`); process.exitCode = 1; });
