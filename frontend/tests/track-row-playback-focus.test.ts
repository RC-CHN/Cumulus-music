import assert from 'node:assert/strict';
import { test } from 'node:test';
import { computed, effectScope, nextTick, ref, shallowRef } from 'vue';
import { usePlaybackTriggerFocus } from '../src/composables/usePlaybackTriggerFocus.ts';

test('row focus waits for new-current disabled edge and final IPC, and yields to navigation', async (t) => {
  const body = {};
  const doc = { body, activeElement: body };
  class Control {
    parentElement = null;
    disabled = false;
    matches() {
      return this.disabled;
    }
    getBoundingClientRect() {
      return { top: 40, bottom: 72 };
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
    const previous = Object.getOwnPropertyDescriptor(globalThis, key);
    Object.defineProperty(globalThis, key, { configurable: true, value });
    t.after(() => {
      if (previous) Object.defineProperty(globalThis, key, previous);
      else Reflect.deleteProperty(globalThis, key);
    });
  }
  const flush = async () => {
    await nextTick();
    await nextTick();
  };
  for (const completion of ['restore', 'other-control', 'replaced-track', 'disposed'] as const) {
    const trigger = new Control();
    const other = new Control();
    const root = shallowRef({ contains: (node: unknown) => node === trigger } as HTMLElement);
    const currentId = ref('81');
    const busy = ref(false);
    const belongs = ref(true);
    const disabled = computed(() => currentId.value === '87' && busy.value);
    const scope = effectScope();
    const focus = scope.run(() =>
      usePlaybackTriggerFocus(
        () => !disabled.value,
        root,
        () => belongs.value,
        () => !busy.value,
      ),
    )!;
    trigger.focus();
    let calls = 0;
    focus.start(() => {
      calls++;
      busy.value = true;
    });
    await flush();
    assert.equal(calls, 1, 'same action sends once');
    currentId.value = '87';
    trigger.disabled = true;
    doc.activeElement = body; // WebView2 blurs when a native event disables the new current row.
    await flush();
    assert.equal(
      doc.activeElement,
      body,
      'do not restore while the actual selection reply is pending',
    );
    if (completion === 'other-control') other.focus();
    if (completion === 'replaced-track') belongs.value = false;
    if (completion === 'disposed') scope.stop();
    busy.value = false;
    trigger.disabled = false;
    await flush();
    assert.equal(
      doc.activeElement,
      completion === 'restore' ? trigger : completion === 'other-control' ? other : body,
      completion,
    );
    assert.equal(calls, 1, 'focus restoration cannot replay playback');
    scope.stop();
  }
});
