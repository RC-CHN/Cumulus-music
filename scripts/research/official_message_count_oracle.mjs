// Optional local replay. Pass the unpacked 3.1.42.205594 module 236.js.
import { readFileSync } from 'node:fs';
import vm from 'node:vm';

const [sourcePath] = process.argv.slice(2);
if (!sourcePath) throw Error('expected the recovered M/236 path');
const source = readFileSync(sourcePath, 'utf8');

function fixture() {
  let now = 0;
  let seq = 0;
  const timers = new Map();
  const reads = [];
  const published = [];
  const mods = {
    15: { Ce: () => {
      reads.push(now);
      return Promise.resolve({ msg: 7 });
    } },
    18: { a: { polling_interval_normal: 240_000, polling_interval_message: 30_000 } },
    32: { u: () => 'sample' },
    319: { a: { next: data => published.push(data) } },
  };
  const sandbox = vm.createContext({
    Date: class extends Date {
      constructor(...args) { super(...(args.length ? args : [now])); }
    },
    setTimeout(callback, delay) {
      const id = ++seq;
      timers.set(id, { at: now + delay, callback });
      return id;
    },
    clearTimeout: id => timers.delete(id),
  });
  const exports = {};
  const requireModule = id => {
    if (!(id in mods)) throw Error(`unexpected dependency M/${id}`);
    return mods[id];
  };
  vm.runInContext(source, sandbox, { timeout: 1000 })({}, exports, requireModule);
  const poll = exports.a;
  function at(end, command) {
    if (end < now) throw Error('clock moved backwards');
    function runTimers() {
      for (;;) {
        const entry = [...timers].filter(([, timer]) => timer.at <= end)
          .sort((a, b) => a[1].at - b[1].at || a[0] - b[0])[0];
        if (!entry) break;
        const [id, timer] = entry;
        timers.delete(id);
        now = timer.at;
        timer.callback();
      }
    }
    runTimers();
    now = end;
    switch (command) {
      case 'bind': poll.switchUser('1'); break;
      case 'logout': poll.switchUser(''); break;
      case 'network-probe': poll.start(true); break;
      case 'open-drawer': poll.switchInterval(true); break;
      case 'close-drawer': poll.switchInterval(false); break;
      case 'direct-refresh': poll.refreshPlCount(); break;
      case undefined: break;
      default: throw Error(`unknown command ${command}`);
    }
    runTimers();
  }
  return { at, reads, published };
}

const bound = fixture();
for (const [time, command] of [
  [0, 'bind'], [6000, 'network-probe'], [240000], [246000, 'open-drawer'],
  [256000, 'direct-refresh'], [276000, 'close-drawer'], [306000],
  [516000], [516001, 'logout'], [760000],
]) bound.at(time, command);
const guest = fixture();
guest.at(5000, 'network-probe');
console.log(JSON.stringify({
  authenticatedReadTimes: bound.reads,
  logoutClearsCounts: bound.published.some(value => value.msg === 0 && value.notice === 0),
  originalGuestReadTimes: guest.reads,
}));
