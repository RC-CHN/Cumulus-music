// Offline replay of the recovered PC 3.1.42.205594 experiment reducers.
import { createHash } from 'node:crypto'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import vm from 'node:vm'

const modules = process.argv[2]
if (!modules || process.argv.length !== 3) {
  throw new Error('usage: node official_pc_experiment_oracle.mjs <recovered hybrid_app module directory>')
}
const text = (id) => readFileSync(join(modules, id + '.js'), 'utf8')
const source = text(147)
const m34 = text(34)
const computed = m34.match(/\bb = "([A-Za-z0-9_-]+)"/)?.[1]
if (computed !== 'PH-PC-playStyleWorldTour') throw new Error('unverified M34 computed key')

const nativeReader = Symbol('native-key-reader')
const request = Symbol('experiment-request')
const logs = Object.freeze({ info() {}, error() {} })
const put = (action) => ({ tag: 'put', action })
put.resolve = put
const effects = { call: (fn, value) => ({ tag: 'call', fn, value }), put }
const sha = (data) => createHash('sha256').update(data).digest('hex')

function model(cached = {}) {
  let saved
  const context = vm.createContext({
    localStorage: {
      getItem(key) {
        if (key !== 'abtest-cached-exps') throw new Error('unexpected cache key')
        return JSON.stringify(cached)
      },
      setItem(key, value) {
        if (key !== 'abtest-cached-exps') throw new Error('unexpected cache key')
        saved = JSON.parse(value)
      },
    },
  }, { codeGeneration: { strings: false, wasm: false } })
  const factory = vm.runInContext(source, context, { timeout: 1000 })
  const imports = {
    2: { b: logs }, 13: { isOSX: false }, 15: { Rf: request },
    34: { b: computed, n: nativeReader }, 150: { e: false }, 161: { a: (fn) => fn },
  }
  const require = (id) => imports[id] ?? {}
  require.d = (exports, key, get) => Object.defineProperty(exports, key, { get })
  const exports = {}
  factory({}, exports, require)
  return { value: exports.a(), saved: () => saved }
}

const m = model()
const read = m.value.effects.refreshAndDispatchAbtest({}, effects)
const pending = read.next().value
if (pending.fn !== nativeReader) throw new Error('not a native-key read')
const nativeFixture = ['synthetic-native-A', 'PH-PC-skipSongs2', 'synthetic-native-B']
const call = read.next(nativeFixture).value
if (call.fn !== request) throw new Error('not an experiment request')
const all = Array.from(call.value)
const open = all.slice(0, -nativeFixture.length)
if (JSON.stringify(all.slice(open.length)) !== JSON.stringify(nativeFixture)) {
  throw new Error('native order changed')
}
const group = (state, key) => state[key]?.expGroupName ?? null
const defaults = Object.entries(m.value.state).map(([key, row]) => [key, row.expGroupName])
const skip = 'PH-PC-skipSongs2'
const vip = 'PH-PC-VIP-songPlay'
const tag = 'PH-PC-tagT'
const cases = [
  { name: 'missing-resets-to-default', rows: [] },
  { name: 'first-row-wins', rows: [{ expName: skip, expGroupName: 'c' }, { expName: skip, expGroupName: 't1' }] },
  { name: 'tag-does-not-disable-skip', rows: [{ expName: skip, expGroupName: 't1' }, { expName: vip, expGroupName: 't1' }] },
  { name: 'tag-off-clears-vip-but-not-skip', rows: [{ expName: tag, expGroupName: 'off' }, { expName: vip, expGroupName: 't1' }, { expName: skip, expGroupName: 't1' }] },
  { name: 'tag-enabled-keeps-vip', rows: [{ expName: tag, expGroupName: 'c' }, { expName: vip, expGroupName: 't1' }] },
  { name: 'non-string-first-match-is-not-t1', rows: [{ expName: skip, expGroupName: 1 }, { expName: skip, expGroupName: 't1' }] },
].map((test) => {
  const independent = model()
  const generation = independent.value.effects.refreshWebAbtest({ payload: { expInfos: test.rows } }, effects)
  for (const effect of generation) {
    if (effect.tag !== 'put') throw new Error('unexpected side effect')
  }
  const saved = independent.saved()
  if (!saved) throw new Error('missing verified cache write')
  return { ...test, skipEnabled: group(saved, skip) === 't1', vipGroup: group(saved, vip) }
})
const restored = model({
  [skip]: { expName: skip, expGroupName: 't1' },
  'PH-PC-qingxv': { expName: 'PH-PC-qingxv', expGroupName: 't1' },
}).value.state
const failed = model().value.effects.refreshAndDispatchAbtest({}, effects)
failed.next()
failed.next(nativeFixture)
const failure = failed.throw(new Error('synthetic connection failure')).value
if (failure.tag !== 'put' || failure.action.type !== 'onUpdate') {
  throw new Error('unexpected refresh failure behavior')
}
console.log(JSON.stringify({
  sources: { m147: sha(source), m34: sha(m34) },
  open, defaults, nativeFixture,
  startup: { skipEnabled: group(restored, skip) === 't1', emotionGroup: group(restored, 'PH-PC-qingxv') },
  failureKeys: Object.keys(failure.action.payload),
  cases,
}, null, 2))
