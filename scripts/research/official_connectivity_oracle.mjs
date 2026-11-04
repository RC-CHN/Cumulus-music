// Local, no-network replay of recovered PC M/119 with the installed Rx vendor.
// Pass the recovered 119.js and original vendors-rudio.pc-new.production.js.
import { readFileSync } from 'node:fs';
import vm from 'node:vm';

const [modulePath, vendorPath] = process.argv.slice(2);
if (!modulePath || !vendorPath) throw Error('expected M/119 and original Rx vendor paths');
const source = readFileSync(modulePath, 'utf8');
const vendor = readFileSync(vendorPath, 'utf8');

function oracle(initialOnline) {
  let now = 0;
  let counter = 0;
  let nativeListener;
  const timers = new Map();
  const listeners = new Map();
  const stable = [];
  const probes = [];
  const arm = (callback, delay, interval, args) => {
    const id = ++counter;
    timers.set(id, { callback, due: now + delay, interval, args });
    return id;
  };
  const navigator = { onLine: initialOnline };
  const context = vm.createContext({
    navigator,
    Date: class extends Date {
      constructor(...args) { super(...(args.length ? args : [now])); }
      static now() { return now; }
    },
    CustomEvent: class {
      constructor(type, options) { this.type = type; this.detail = options?.detail; }
    },
    addEventListener(name, handler) {
      if (!listeners.has(name)) listeners.set(name, new Set());
      listeners.get(name).add(handler);
    },
    removeEventListener(name, handler) { listeners.get(name)?.delete(handler); },
    dispatchEvent(event) {
      for (const handler of [...(listeners.get(event.type) ?? [])]) handler(event);
    },
    setTimeout: (cb, delay = 0, ...args) => arm(cb, delay, 0, args),
    clearTimeout: id => timers.delete(id),
    setInterval: (cb, delay, ...args) => arm(cb, delay, delay, args),
    clearInterval: id => timers.delete(id),
  });
  context.window = context;
  context.self = context;
  vm.runInContext(vendor, context, { timeout: 3000 });
  const modules = {
    2: { b: { info() {} } },
    4: {
      Network: { subscribe: callback => { nativeListener = callback; } },
      App: { getLocalConfig: async () => '{}' },
    },
    22: context.RxOperators,
    25: context.Rx,
    236: { a: { start: immediate => probes.push([now, immediate]) } },
  };
  const requireModule = id => {
    if (!(id in modules)) throw Error(`unexpected dependency M/${id}`);
    return modules[id];
  };
  requireModule.d = (target, key, get) => Object.defineProperty(target, key, { get });
  const exports = {};
  vm.runInContext(source, context, { timeout: 1000 })({}, exports, requireModule);
  exports.a.subscribe(value => stable.push([now, value]));
  const to = (end, event) => {
    if (end < now) throw Error('clock moved backwards');
    while (true) {
      const entry = [...timers].filter(([, timer]) => timer.due <= end)
        .sort((a, b) => a[1].due - b[1].due || a[0] - b[0])[0];
      if (!entry) break;
      const [id, timer] = entry;
      now = timer.due;
      if (timer.interval) timer.due += timer.interval;
      else timers.delete(id);
      timer.callback(...timer.args);
    }
    now = end;
    switch (event) {
      case 'native-off': nativeListener(false); break;
      case 'native-on': nativeListener(true); break;
      case 'browser-off':
      case 'browser-on':
        navigator.onLine = event === 'browser-on';
        context.dispatchEvent({ type: navigator.onLine ? 'online' : 'offline' });
        break;
      case 'request-off':
      case 'request-on':
        context.dispatchEvent({ type: event === 'request-on' ? 'musicFeOnline' : 'musicFeOffline',
          detail: 'request' });
        break;
      case undefined: break;
      default: throw Error(`unknown signal ${event}`);
    }
    return [now, exports.b()];
  };
  return { to, stable, probes };
}

const scenarios = [
  ['strict-five-seconds', true, [
    [100, 'request-off'], [199, 'request-off'], [5100, 'request-off'],
    [5101, 'request-off'], [5200, 'request-off'], [5300, 'browser-on'],
    [5400, 'request-on'], [6300], [10200],
  ]],
  ['navigator-offline', true, [
    [100, 'browser-off'], [5100], [10100], [15100], [20100],
    [20200, 'browser-on'], [21200], [21250, 'request-on'],
  ]],
  ['native-false', false, [
    [10, 'native-off'], [110, 'browser-on'], [1010], [1110], [1120, 'request-on'],
  ]],
  ['throttles-reset-on-confirmation', true, [
    [100, 'request-off'], [5101, 'request-off'], [5102, 'request-on'],
    [5103, 'request-off'], [5104, 'request-on'], [10101],
  ]],
  ['online-success-keeps-browser-offline-probe', true, [
    [100, 'browser-off'], [200, 'request-on'], [5100], [10100], [15100],
  ]],
  ['pre-online-probe-cancelled-by-window-offline', false, [
    [100, 'browser-on'], [200, 'browser-off'], [1100], [3000, 'browser-on'],
    [4000], [4010, 'request-on'], [5200],
  ]],
];
const outputs = scenarios.map(([name, initial, actions]) => {
  const harness = oracle(initial);
  const samples = actions.map(([time, event]) => harness.to(time, event));
  return { name, initial, actions: actions.map(([time, event]) => [time, event ?? null]),
    samples, stable: harness.stable, probes: harness.probes };
});
process.stdout.write(`${JSON.stringify(outputs, null, 2)}\n`);
