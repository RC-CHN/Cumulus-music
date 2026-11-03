import assert from 'node:assert/strict';
import test from 'node:test';
import { createPageTransition } from '../src/services/pageTransition.ts';

class FakeElement extends EventTarget {
  readonly classes = new Set<string>();
  readonly classList = {
    add: (...names: string[]) => names.forEach((name) => this.classes.add(name)),
    remove: (...names: string[]) => names.forEach((name) => this.classes.delete(name)),
  };
  getBoundingClientRect() {}
  end(propertyName: string) {
    this.dispatchEvent(Object.assign(new Event('transitionend'), { propertyName }));
  }
}

function setup(reduced = false) {
  let id = 0;
  const frames = new Map<number, () => void>();
  const deadlines = new Map<number, () => void>();
  const element = new FakeElement();
  const hooks = createPageTransition(() => reduced, {
    frame: (run) => {
      frames.set(++id, run);
      return id;
    },
    cancelFrame: (token) => frames.delete(token),
    deadline: (run) => {
      deadlines.set(++id, run);
      return id;
    },
    cancelDeadline: (token) => deadlines.delete(token),
  });
  return { hooks, element, target: element as unknown as Element, frames, deadlines };
}

test('page leave completes even if its compositor frame never arrives', () => {
  const { hooks, element, target, frames, deadlines } = setup();
  let ended = 0;
  hooks.leave(target, () => ended++);
  assert.ok(element.classes.has('page-leave-active'));
  const staleFrame = [...frames.values()][0]!;
  const deadline = [...deadlines.values()][0]!;
  deadline();
  staleFrame();
  deadline();
  assert.equal(ended, 1);
  assert.equal(element.classes.size, 0);
  assert.equal(frames.size, 0);
});

test('a real page transform ends the animation before the deadline', () => {
  const { hooks, element, target, frames, deadlines } = setup();
  let ended = 0;
  hooks.enter(target, () => ended++);
  assert.ok(element.classes.has('page-enter-from'));
  [...frames.values()][0]!();
  assert.ok(!element.classes.has('page-enter-from'));
  element.end('opacity');
  assert.equal(ended, 0);
  element.end('transform');
  assert.equal(ended, 1);
  assert.equal(deadlines.size, 0);
  assert.equal(element.classes.size, 0);
});

test('cancelled page transition cannot complete over a newer one', () => {
  const { hooks, element, target, deadlines } = setup();
  let old = 0,
    current = 0;
  hooks.enter(target, () => old++);
  const oldDeadline = [...deadlines.values()][0]!;
  hooks.cancel(target);
  hooks.leave(target, () => current++);
  oldDeadline();
  assert.equal(old, 0);
  assert.ok(element.classes.has('page-leave-active'));
  [...deadlines.values()][0]!();
  assert.equal(current, 1);
});

test('reduced motion completes after the current patch without frames or timers', async () => {
  const { hooks, element, target, frames, deadlines } = setup(true);
  let ended = 0;
  hooks.enter(target, () => ended++);
  assert.equal(ended, 0);
  await Promise.resolve();
  assert.equal(ended, 1);
  hooks.leave(target, () => assert.fail('cancelled reduced-motion transition'));
  hooks.cancel(target);
  hooks.leave(target, () => ended++);
  await Promise.resolve();
  assert.equal(ended, 2);
  assert.equal(frames.size + deadlines.size + element.classes.size, 0);
});
