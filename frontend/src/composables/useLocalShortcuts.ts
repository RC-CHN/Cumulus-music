import { watch } from 'vue';
import { failure, type CoreFailure } from '../services/contracts.ts';
import {
  eventChord,
  type ShortcutKeysClient,
  type ShortcutLease,
  type ShortcutSelection,
} from '../services/shortcuts.ts';
import { useShortcutStatus } from './useShortcutStatus.ts';
import { useCore } from '../services/core.ts';
export function useLocalShortcuts(
  notice: (error: CoreFailure) => void,
  core: ShortcutKeysClient = useCore(),
) {
  const native = useShortcutStatus(core);
  let epoch = '',
    lease: ShortcutLease | undefined,
    attaching = '',
    serial = 0,
    disposed = false,
    connected = false,
    busy = false;
  let unlisten: (() => void) | undefined;
  function detach() {
    const old = lease;
    lease = undefined;
    busy = false;
    if (old) void core.shortcutDetach(old.id).catch(() => {});
  }
  async function attach() {
    const value = native.view.value;
    if (disposed || !connected || !epoch || value?.phase !== 'ready' || !value.ticket) {
      detach();
      return;
    }
    const owner = epoch,
      ticket = value.ticket,
      key = `${owner}:${ticket}`;
    if (
      (lease?.ticket === ticket &&
        value.rows.some((r) => r.scope === 'local' && r.status.kind === 'active')) ||
      attaching === key
    )
      return;
    detach();
    attaching = key;
    const current = ++serial;
    try {
      const result = await core.shortcutAttach(owner, ticket);
      if (
        disposed ||
        current !== serial ||
        owner !== epoch ||
        native.view.value?.ticket !== ticket
      ) {
        void core.shortcutDetach(result.id).catch(() => {});
        return;
      }
      lease = result;
    } catch (e) {
      if (!disposed && current === serial && owner === epoch && failure(e).code !== 'stale')
        notice(failure(e));
    } finally {
      if (attaching === key) attaching = '';
    }
  }
  const stopWatch = watch(native.view, () => void attach(), { flush: 'sync' });
  function account(value: string) {
    if (value === epoch) return;
    epoch = value;
    serial++;
    attaching = '';
    detach();
    busy = false;
    void attach();
  }
  async function connect(value: string) {
    account(value);
    connected = true;
    try {
      const stop = await core.onShortcutError((value) => {
        if (!disposed && value.epoch === epoch) notice(value.error);
      });
      if (disposed) {
        stop();
        return;
      }
      unlisten = stop;
    } catch (e) {
      if (!disposed) notice(failure(e));
    }
    if (!disposed) await native.connect();
  }
  function handle(event: KeyboardEvent, blocked: boolean, selection: ShortcutSelection): boolean {
    if (
      blocked ||
      disposed ||
      event.defaultPrevented ||
      event.repeat ||
      event.isComposing ||
      event.metaKey ||
      document.querySelector('dialog[open],[aria-modal="true"]')
    )
      return false;
    const target = event.target;
    if (
      !(target instanceof HTMLElement) ||
      target.isContentEditable ||
      target.closest('textarea,select,[role="textbox"],[contenteditable="true"]') ||
      (target.closest('input') && !(event.ctrlKey || event.altKey || event.shiftKey)) ||
      target.closest('input:not([type="range"]):not([type="checkbox"]):not([type="radio"])') ||
      (target.closest('button,a,[role="button"]') &&
        event.code === 'Space' &&
        !event.ctrlKey &&
        !event.altKey &&
        !event.shiftKey)
    )
      return false;
    const keys = eventChord(event),
      active = lease,
      value = native.view.value;
    if (!keys || !active || value?.phase !== 'ready' || active.ticket !== value.ticket)
      return false;
    if (
      !value.rows.some(
        (r) =>
          r.scope === 'local' &&
          r.status.kind === 'active' &&
          r.chord.length === keys.length &&
          r.chord.every((v, i) => v === keys[i]),
      )
    )
      return false;
    event.preventDefault();
    if (busy) return true;
    busy = true;
    const owner = epoch,
      current = serial;
    void core
      .shortcutPress({ epoch: owner, lease: active.id, ticket: active.ticket, keys, selection })
      .catch((e) => {
        if (!disposed && owner === epoch && failure(e).code !== 'stale') notice(failure(e));
      })
      .finally(() => {
        if (current === serial) busy = false;
      });
    return true;
  }
  function dispose() {
    disposed = true;
    serial++;
    stopWatch();
    detach();
    native.dispose();
    unlisten?.();
  }
  return { native, connect, account, handle, dispose };
}
