// Local no-egress replay of the extracted M/308, M/1384 and M/176 decisions.
// Arguments are the recovered 308.js, 1384.js and 176.js from the same chunk.
import { readFileSync } from 'node:fs';
import vm from 'node:vm';

const [playingPath, listPath, selectorPath] = process.argv.slice(2);
if (!selectorPath) throw Error('expected M/308, M/1384 and M/176 paths');
const playing = readFileSync(playingPath, 'utf8');
const list = readFileSync(listPath, 'utf8');
const selector = readFileSync(selectorPath, 'utf8');

function method(source, start, next, last = false) {
  const from = last ? source.lastIndexOf(start) : source.indexOf(start);
  const to = source.indexOf(next, from + 1);
  if (from < 0 || to < 0) throw Error('recovered module markers changed');
  return `({${source.slice(from, to)}})`;
}
const scenes = { native: 'native', miniBar: 'miniBar', playingList: 'playingList',
  playlist: 'playlist', petDesktop: 'petDesktop', search: 'search',
  fm: 'fm', zoneFm: 'zoneFm', local: 'local' };
const types = { voice: 'voice', track: 'track', downloadTrack: 'downloadTrack' };
const modes = { PlayOrder: 'playOrder', PlayRandom: 'playRandom', PlayOneCycle: 'playOneCycle',
  playCycle: 'playCycle', PlayAi: 'playAi', PlayFm: 'playFm' };
const states = { Playing: 'playing', Stop: 'stop', End: 'end' };
function extreme(rows, score, greater) {
  return rows.reduce((best, row) => !best || (greater ? score(row) > score(best) : score(row) < score(best))
    ? row : best, undefined);
}
function harness({ assigned = 't1', mode = modes.PlayOrder, scene, cause, flag, current, attempted, candidates, lowerAllowed, nativeEnd = false }) {
  const rows = ['1', '2', '3', '4'].map((id, displayOrder) => ({
    id, resourceId: id, trackId: id, resourceType: 'track', displayOrder, randomOrder: [0, 3, 1, 2][displayOrder],
    track: { id }, scene: scenes.playlist,
  }));
  const store = {
    abtest: { 'PH-PC-skipSongs2': { expGroupName: assigned } },
    app: { isOnline: true },
    host: { isAlbumVip: false, isVinylVip: false, isPlusVip: false },
    playing: { playingMode: mode, curPlaying: rows[current - 1], playingState: states.Playing, playId: 'old-play' },
    playingList: { curPlayingList: rows },
  };
  const actions = [];
  const log = { b: { info() {} } };
  const local = {
    _: log, S: log, ie: { a: { info() {} } }, Qe: { a: { info() {} } },
    m: { a: scenes }, y: { m: types, i: modes, k: states, p: 'playingList' }, ee: { i: modes },
    k: { m: item => item.track, j: () => ({ canPlayList: candidates.map(id => ({ id })) }), h: () => null },
    He: { cur: 0, next: 1 }, a: { a: (rows, score) => extreme(rows, score, true) }, se: { a: {} }, f: { c: () => false },
    u: { L: { create: () => actions.push(['notice']), error() {} } },
    Ie: (left, right) => ({ ...left, ...right }),
    Lt: (left, right) => ({ ...left, ...right }),
    Ee: '', xt: 0, St: 0, ht: { a: { next() {} } },
    Ot: { f: () => false }, rt: { a: (rows, key) => rows.find(row => row.resourceId === key.resourceId) },
    r: { a: value => value == null || Object.keys(value).length === 0 },
    R: { d: song => {
      actions.push(['lower-rights', song.id]);
      return lowerAllowed?.includes(song.id);
    } },
  };
  const ctx = vm.createContext(local);
  const mods = {
    4: {}, 536: { a: (rows, pred) => rows.filter(pred) }, 534: { a: rows => rows.length === 0 },
    751: { a: (rows, score) => extreme(rows, score, false) },
    752: { a: (rows, score) => extreme(rows, score, true) },
    1404: {}, 21: {}, 7: { i: modes }, 213: {}, 8: {},
  };
  const req = id => {
    if (!(id in mods)) throw Error(`unexpected selector dependency ${id}`);
    return mods[id];
  };
  req.d = (target, key, get) => Object.defineProperty(target, key, { get });
  const dt = {};
  vm.runInContext(selector, ctx, { timeout: 1000 })({}, dt, req);
  ctx.dt = dt;
  const play = vm.runInContext(method(playing, 'playOneTrackInPlayingList(e, t) {',
    'handleNativePlayError(e, t) {'), ctx, { timeout: 1000 }).playOneTrackInPlayingList;
  const jump = vm.runInContext(method(list, 'jump2Track(e, t) {',
    'refreshRandomPlayingList(e, t) {', true), ctx, { timeout: 1000 }).jump2Track;
  const playingAction = playing.slice(playing.indexOf('setPlaying(e, t) {'));
  const lowerStart = playingAction.search(/let F,\s+U = null,/);
  const lowerEnd = playingAction.indexOf('if (N && N.resourceId', lowerStart);
  if (lowerStart < 0 || lowerEnd < 0) throw Error('recovered lower privilege markers changed');
  const lower = vm.runInContext(`(function* (e,t,d,h,f,l,a) {
    const S=false,T=false,P=false,M=false;
    ${playingAction.slice(lowerStart, lowerEnd)}
  })`, ctx, { timeout: 1000 });
  function execute(generator) {
    let input;
    for (let n = 0; n < 50; n++) {
      const step = generator.next(input);
      if (step.done) return;
      const effect = step.value;
      input = effect?.op === 'put' ? dispatch(effect.action) : effect;
    }
    throw Error('unbounded recovered effect');
  }
  const put = action => ({ op: 'put', action });
  put.resolve = put;
  const env = { put, select: fn => fn(store), call: (fn, ...args) => fn(...args) };
  function dispatch(action) {
    const p = action.payload;
    switch (action.type) {
      case 'onUpdate':
        Object.assign(store.playing, p);
        actions.push(['update', p]);
        break;
      case 'checkPlayPrivilege':
        actions.push(['check', p.resource.id]);
        return false;
      case 'playingList/jump2Track':
        actions.push(['jump', p.type, p.triggerScene, p.flag, p.assignedResourceId ?? null]);
        execute(jump(action, env));
        break;
      case 'playing/playOneTrackInPlayingList':
        actions.push(['attempt', p.item.resourceId, p.triggerScene]);
        execute(play(action, env));
        break;
      case 'pause': actions.push(['pause', p.triggerScene]); break;
      case 'setPlaying':
        actions.push(['set', p.trackIn.resourceId, p.playingState, p.triggerScene]);
        if (lowerAllowed && p.playingState === states.Playing && p.triggerScene === scenes.playlist)
          execute(lower(p.trackIn, p.playingState, p.flag, p.triggerScene, false, put, env.call));
        break;
      default: throw Error(`unexpected recovered dispatch ${action.type}`);
    }
  }
  if (nativeEnd) {
    // Run the actual final M308 onPlayEnd switch and its post-switch update.
    // Earlier media, analytics and reducer effects are outside this replay.
    const start = playing.indexOf('const {\n              reason: ne\n            } = X;');
    const end = playing.indexOf('          }();', start);
    if (start < 0 || end < 0) throw Error('recovered onPlayEnd markers changed');
    const tail = vm.runInContext('(function* (e,t,o,X,l,s,r) {' +
      playing.slice(start, end) + '})', ctx, { timeout: 1000 });
    execute(tail(rows[current - 1], mode, { triggerScene: scene }, { reason: 'end' },
      env.select, put, env.call));
    actions.push(['final-play-id', store.playing.playId]);
  } else {
    execute(play({ payload: { item: rows[attempted - 1], flag, switchType: cause, triggerScene: scene } }, env));
  }
  return actions;
}

const common = { current: 1, attempted: 2, flag: 1 };
const cycle = { nativeEnd: true, scene: scenes.native, current: 4,
  candidates: ['2', '3', '4'], lowerAllowed: ['2', '3', '4'], mode: modes.playCycle };
const random = { ...cycle, current: 2, mode: modes.PlayRandom };
const cases = {
  minibarRevisitsDenied: { ...common, scene: scenes.miniBar, cause: 'call', candidates: ['1', '4'] },
  minibarRetriesAssignedRows: { ...common, scene: scenes.miniBar, cause: 'call',
    candidates: ['1', '4'], lowerAllowed: ['1', '4'] },
  noCandidatesPause: { ...common, scene: scenes.miniBar, cause: 'call', candidates: [] },
  substituteAbsentStops: { ...common, scene: scenes.native, cause: 'end', candidates: ['1', '90'] },
  substitutePresentSteps: { ...common, scene: scenes.native, cause: 'end', candidates: ['1', '3'] },
  directQueueStopsBeforeScan: { ...common, scene: scenes.playingList, cause: 'call', flag: 0, candidates: ['4'] },
  explicitSearchOutsideGate: { ...common, scene: scenes.search, cause: 'call', flag: 0, candidates: ['4'] },
  sequentialControlReturnsWithoutClearingPlayId: { ...common, assigned: 'c', nativeEnd: true, scene: scenes.native, candidates: ['1', '4'] },
  loopControlClearsPlayIdAfterDeniedWrap: { ...cycle, assigned: 'c' },
  randomControlClearsPlayIdAfterDeniedWrap: { ...random, assigned: 'c' },
  loopTreatmentWrapsByDisplayOrder: cycle,
  randomTreatmentWrapsByRandomOrder: random,
};
const results = Object.fromEntries(Object.entries(cases).map(([name, input]) => [name, harness(input)]));
console.log(JSON.stringify(results, null, 2));
