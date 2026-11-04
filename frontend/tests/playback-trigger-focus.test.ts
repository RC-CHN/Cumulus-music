import assert from 'node:assert/strict';
import { test } from 'node:test';
import { effectScope, nextTick, ref, shallowRef } from 'vue';
import { usePlaybackTriggerFocus } from '../src/composables/usePlaybackTriggerFocus.ts';

test('search cover focus survives transient native transport while the selection is pending', async (t) => {
  const body = {};
  const doc = { body, activeElement: body };
  class Control {
    parentElement = null;
    matches() {
      return false;
    }
    getBoundingClientRect() {
      return { top: 20, bottom: 100 };
    }
    focus() {
      doc.activeElement = this;
    }
  }
  for (const [key, value] of Object.entries({
    document: doc,
    HTMLElement: Control,
    innerHeight: 500,
  })) {
    const descriptor = Object.getOwnPropertyDescriptor(globalThis, key);
    Object.defineProperty(globalThis, key, { configurable: true, value });
    t.after(() => {
      if (descriptor) Object.defineProperty(globalThis, key, descriptor);
      else Reflect.deleteProperty(globalThis, key);
    });
  }
  const cover = new Control();
  const other = new Control();
  const root = shallowRef({ contains: (node: unknown) => node === cover } as HTMLElement);
  const ready = ref(true);
  const settled = ref(true);
  const scope = effectScope();
  t.after(() => scope.stop());
  const focus = scope.run(() =>
    usePlaybackTriggerFocus(
      () => ready.value,
      root,
      () => true,
      () => settled.value,
    ),
  )!;
  const flush = async () => {
    await nextTick();
    await nextTick();
  };

  for (const selectOther of [false, true]) {
    cover.focus();
    focus.start(() => {
      settled.value = false;
      ready.value = false;
      doc.activeElement = body; // A disabled WebKit button can lose focus.
    });
    await flush();
    ready.value = true; // Pending native transport becomes interactive.
    await flush();
    assert.equal(doc.activeElement, body, 'do not consume the origin at intermediate readiness');
    ready.value = false; // Native playback starts before the IPC acknowledgement.
    await flush();
    if (selectOther) other.focus();
    settled.value = true;
    ready.value = true;
    await flush();
    assert.equal(doc.activeElement, selectOther ? other : cover);
  }
});
