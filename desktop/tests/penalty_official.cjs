// Optional offline oracle. Run only when recovered official sources are present.
// Executes complete M1368 methods with synthetic platform I/O; no SDK bootstrap,
// network, real account, password mutation, binding, or certification submission.
const fs = require('node:fs');
const vm = require('node:vm');
const assert = require('node:assert/strict');
const crypto = require('node:crypto');
const parser = require('../../frontend/node_modules/@babel/parser');
const fixturePath = 'core/tests/fixtures/penalty-login-cases.json';
const fixture = JSON.parse(fs.readFileSync(fixturePath, 'utf8'));
const sourcePath = 'references/netease-3.1.42.205594-analysis/recovered-js/modules/' + fixture.source;
const source = fs.readFileSync(sourcePath, 'utf8');
const sha256 = crypto.createHash('sha256').update(source).digest('hex');
assert.equal(sha256, fixture.sourceSha256);
const tree = parser.parse(source);
function walk(node, visit, parent) {
  if (!node || typeof node !== 'object') return;
  visit(node, parent);
  for (const value of Object.values(node)) {
    if (Array.isArray(value)) value.forEach(child => walk(child, visit, node));
    else if (value && typeof value === 'object') walk(value, visit, node);
  }
}
const methods = {};
const wanted = ['handleLoginResponse', 'handleVerifyResponse', 'notifyPenaltyStepComplete', 'syncPenaltyExtraInfo'];
let block;
walk(tree, (node, parent) => {
  if (node.type === 'FunctionDeclaration' && node.id.name === 'O') block = parent;
  if (node.type !== 'ObjectExpression') return;
  const name = node.properties.find(p => p.key?.name === 'key')?.value?.value;
  const fn = node.properties.find(p => p.key?.name === 'value')?.value;
  if (!fn) return;
  const text = source.slice(fn.start, fn.end);
  if (wanted.includes(name)) {
    // KernelServiceLogin also delegates notifyPenaltyStepComplete; execute the
    // actual handler, whose native continuation calls handleVerifyResponse.
    if (name === 'notifyPenaltyStepComplete' && !text.includes('penalty step completed')) return;
    assert(!methods[name], `ambiguous original method ${name}`);
    methods[name] = text;
  }
  if (name === 'fetch' && text.includes('fromPage: "JSK"')) {
    assert(!methods.fetch, 'ambiguous PC platform fetch');
    methods.fetch = text;
  }
});
assert(block);
for (const name of [...wanted, 'fetch']) assert(methods[name], name);
const helpers = block.body
  .filter(node => node.type === 'FunctionDeclaration')
  .map(node => source.slice(node.start, node.end))
  .join('\n');
const plain = value => JSON.parse(JSON.stringify(value));
function environment(extra = {}) {
  const context = {
    Ve: { log() {}, error() {} },
    // M1368's scene constants used by the original dn/vn helpers.
    ln: 0,
    hn: 1,
    Ue: {
      NEED_VERIFY: 'NEED_VERIFY',
      NEED_MORE_VERIFY: 'NEED_MORE_VERIFY',
      LOGIN_FAILED: 'LOGIN_FAILED',
      VERIFY_SUCCESS: 'VERIFY_SUCCESS',
      REPLAY_LOGIN_SUCCESS: 'REPLAY_LOGIN_SUCCESS',
    },
    t: { addVerifyStep() {}, resetVerifySteps() {}, getVerifyStepsResponse() { return ''; } },
    ...extra,
  };
  vm.createContext(context);
  vm.runInContext(
    helpers + '\nJe=function(){};globalThis.methods={' +
      Object.entries(methods).map(([name, method]) => `${name}:(${method})`).join(',') + '};',
    context,
    { timeout: 1000 },
  );
  return context;
}
function moreVerifyType(type) {
  return {
    BIND_PHONE: 'penalty_bindPhone',
    REAL_NAME: 'penalty_realname',
    UPGRADE_WEAK_PASSWORD: 'penalty_upgradePassword',
  }[type] || 'penalty_' + type;
}
(async () => {
  const cases = [];
  for (const row of fixture.cases) {
    const context = environment();
    const entryEvents = [];
    const response = { body: { code: 8831, data: { steps: row.steps, musicTuP: 'synthetic-penalty-ticket' } } };
    const entry = await context.methods.handleLoginResponse.call(
      { eventEmitter: { emit(event) { entryEvents.push(plain(event)); } } },
      response,
      { type: 'PHONE_PASSWORD', phone: '13800000000', countryCode: '86' },
      false,
      false,
      { scene: '0' },
    );
    assert.equal(entry.code, 8831);
    assert.equal(entry.success, false);
    assert.equal(entryEvents.length, 1);
    const entryEvent = entryEvents[0];
    const upgradeRequired = row.entryPhase === 'upgrade_required';
    assert.equal(entryEvent.type, upgradeRequired ? 'LOGIN_FAILED' : 'NEED_VERIFY', row.name);
    const route = new URL(entryEvent.payload.routeUrl);
    assert.equal(route.searchParams.get('component'), 'rn-account-verify');
    if (upgradeRequired) {
      assert.equal(route.searchParams.get('route'), 'updateDialog');
      const step = row.steps[row.entryCurrent];
      assert.equal(route.searchParams.get('forceUpdateVersion'), step.minVersion ? String(step.minVersion) : null);
    } else {
      assert.equal(route.searchParams.get('subScene'), 'penalty');
      assert.equal(route.searchParams.get('musicTuP'), 'synthetic-penalty-ticket');
      const identity = row.steps.find(step => step.type === 'IDENTITY_VERIFY');
      assert.equal(route.searchParams.get('forceUpdateVersion'), identity?.minVersion ? String(identity.minVersion) : null);
    }

    const calls = [], events = [], replays = [];
    const nativeState = {
      getVerifyType: () => 'PENALTY',
      needMoreVerify: () => '',
      getPlatform: () => ({ fetch: async (path, payload, method, options) => {
        calls.push(plain({ path, payload, method, options }));
        return { body: { code: 200, data: { steps: row.steps } } };
      } }),
      eventEmitter: { emit(event) { events.push(plain(event)); } },
      replayLoginAfterVerify: async extraInfo => {
        replays.push(plain(extraInfo));
        return { success: true, code: 200 };
      },
    };
    await context.methods.handleVerifyResponse.call(
      nativeState,
      { body: { code: 200 } },
      { subScene: 'penalty', scene: '0' },
      '0',
    );
    assert.deepEqual(calls, [{ path: '/api/login/penalty/status', payload: {}, method: 'POST', options: {} }]);
    const more = events.find(event => event.type === 'NEED_MORE_VERIFY');
    const outcome = more ? 'next_step' : replays[0]?.ignorePenalty ? 'identity_pending' : 'all_passed';
    assert.equal(outcome, row.statusOutcome, row.name);
    if (outcome === 'next_step') {
      assert.equal(replays.length, 0);
      assert.equal(more.payload.moreVerifyType, moreVerifyType(row.steps[row.statusCurrent].type));
      assert.equal(more.payload.hasReplayLogin, false);
    } else {
      assert.equal(replays.length, 1);
      assert.equal(Boolean(replays[0].ignorePenalty), outcome === 'identity_pending');
    }
    // Record that the original does replay. The Rust user-triggered status
    // action intentionally only reads status/identity and never submits it.
    cases.push({ name: row.name, entryEvent: entryEvent.type, statusOutcome: outcome, calls, replays });
  }

  const ignoredEvents = [];
  const ignored = await environment().methods.handleLoginResponse.call(
    { eventEmitter: { emit(event) { ignoredEvents.push(event); } } },
    { body: { code: 8831, data: { steps: [] } } },
    { type: 'PHONE_PASSWORD' },
    false,
    false,
    { ignorePenalty: true },
  );
  assert.equal(ignored.success, false);
  assert.equal(ignored.code, 8831);
  assert.equal(ignoredEvents.length, 0);

  // Original status handling defaults malformed/missing steps to []. The
  // Rust model deliberately rejects these responses instead of treating them
  // as completion. Capture this divergence rather than claiming equivalence.
  const hardeningDifferences = [];
  for (const [name, response] of [
    ['missing_steps', { body: { code: 200, data: {} } }],
    ['service_rejection', { body: { code: 500, data: {} } }],
  ]) {
    let replayCount = 0;
    const context = environment();
    await context.methods.handleVerifyResponse.call(
      {
        getVerifyType: () => 'PENALTY',
        needMoreVerify: () => '',
        getPlatform: () => ({ fetch: async () => response }),
        replayLoginAfterVerify: async () => { replayCount++; return { success: true }; },
        eventEmitter: { emit() {} },
      },
      { body: { code: 200 } },
      { subScene: 'penalty' },
      '0',
    );
    assert.equal(replayCount, 1);
    hardeningDifferences.push({ name, originalReplayCount: replayCount, rustRequiresValidStatus: true });
  }

  const continuationCalls = [];
  let extraInfo = { subScene: 'penalty', scene: '0', password: 'synthetic-old-password' };
  const context = environment({ Yn: { getInstance: () => ({ getExtraInfo: () => extraInfo }) } });
  const manager = {
    getExtraInfo: () => extraInfo,
    setExtraInfo: value => { extraInfo = value; },
  };
  await context.methods.syncPenaltyExtraInfo.call(
    { verifyManager: manager },
    { password: 'synthetic-updated-password' },
  );
  assert.equal(extraInfo.password, 'synthetic-updated-password');
  await context.methods.notifyPenaltyStepComplete.call(
    { handleVerifyResponse: async (...args) => continuationCalls.push(plain(args)) },
    'UPGRADE_WEAK_PASSWORD',
  );
  assert.equal(continuationCalls.length, 1);
  assert.equal(continuationCalls[0][0].body.code, 200);
  assert.equal(continuationCalls[0][1].password, 'synthetic-updated-password');
  extraInfo = { subScene: 'not-penalty' };
  await context.methods.notifyPenaltyStepComplete.call(
    { handleVerifyResponse: async () => assert.fail('foreign scene must not continue') },
    'REAL_NAME',
  );

  const requests = [];
  const adapter = environment({
    We: () => '',
    Yn: { getInstance: () => ({ getMusicTu: () => 'synthetic-tu-ticket' }) },
    o: { Network: { nativeRequest: async (path, options) => {
      requests.push(plain({ path, options }));
      return { body: { code: 200, data: { steps: [] } } };
    } } },
  });
  await adapter.methods.fetch('/api/login/penalty/status', {}, 'POST', {});
  assert.equal(requests.length, 1);
  assert.deepEqual(requests[0].options.data, { fromPage: 'JSK' });
  assert.deepEqual(requests[0].options.header, { 'x-login-chain-id': '', 'x-music-tu': 'synthetic-tu-ticket' });
  assert(!JSON.stringify(requests).includes('musicTuP'));

  const report = {
    schema: 1,
    source: sourcePath,
    sha256,
    scope: 'Complete original entry/status/continuation/PC-fetch methods with synthetic dependencies; RN write actions remain unrecovered.',
    cases,
    ignorePenaltyIsStillFailure: true,
    hardeningDifferences,
    continuation: { calls: continuationCalls.length, updatedPasswordWasNativeOnly: true },
    statusRequest: requests[0],
  };
  const output = JSON.stringify(report, null, 2) + '\n';
  if (process.argv[2]) fs.writeFileSync(process.argv[2], output);
  else process.stdout.write(output);
})().catch(error => { console.error(error); process.exitCode = 1; });
