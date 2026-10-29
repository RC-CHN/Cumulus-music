import { computed, ref, shallowRef } from 'vue';
import { failure, type AccountData, type AccountView } from '../services/contracts.ts';
import type {
  ShortcutAction,
  ShortcutScope,
  ShortcutEdit,
  ShortcutCommand,
  ShortcutSettingsView,
  ShortcutPreferencesClient,
  KeyInput,
  ShortcutStatus,
} from '../services/shortcuts.ts';
import { useCore } from '../services/core.ts';
export interface ShortcutDraft {
  action: ShortcutAction;
  scope: ShortcutScope;
  id: string | null;
  keys: number[];
  revision: string;
  changed: boolean;
}
export function useShortcutSettings(core: ShortcutPreferencesClient = useCore()) {
  const view = shallowRef<ShortcutSettingsView>(),
    draft = shallowRef<ShortcutDraft>();
  const loading = ref(false),
    busy = ref(false),
    error = ref(''),
    saved = ref(false),
    recordError = ref('');
  let epoch: string | undefined,
    serial = 0,
    recordSerial = 0,
    captureSerial = 0,
    disposed = false;
  const canChange = computed(
    () =>
      !!view.value?.current &&
      !view.value.damaged &&
      !loading.value &&
      !busy.value &&
      !error.value &&
      !draft.value,
  );
  function accept(result: AccountData<ShortcutSettingsView>, owner: string) {
    if (result.epoch !== owner)
      throw { code: 'stale', message: '账号已变化，请重新读取快捷键设置' };
    view.value = result.value;
  }
  async function read() {
    if (disposed || !epoch || busy.value || draft.value) return;
    const owner = epoch,
      current = ++serial,
      active = () => !disposed && owner === epoch && current === serial;
    loading.value = true;
    error.value = '';
    saved.value = false;
    try {
      const result = await core.shortcutSettings(owner);
      if (active()) accept(result, owner);
    } catch (e) {
      if (active()) error.value = failure(e).message;
    } finally {
      if (active()) loading.value = false;
    }
  }
  async function cancelRecording() {
    const id = draft.value?.id,
      owner = epoch,
      current = ++recordSerial;
    draft.value = undefined;
    captureSerial++;
    recordError.value = '';
    if (id)
      try {
        await core.shortcutEndRecording(id);
      } catch (e) {
        if (!disposed && owner === epoch && current === recordSerial)
          error.value = failure(e).message;
      }
  }
  function connect(account: AccountView) {
    if (epoch === account.epoch) return;
    void cancelRecording();
    epoch = account.epoch;
    serial++;
    view.value = undefined;
    loading.value = false;
    busy.value = false;
    error.value = '';
    saved.value = false;
    return read();
  }
  async function command(command: ShortcutCommand) {
    if (disposed || !epoch || busy.value || loading.value) return false;
    const owner = epoch,
      current = ++serial,
      active = () => !disposed && owner === epoch && current === serial;
    busy.value = true;
    error.value = '';
    saved.value = false;
    try {
      const result = await core.shortcutSettingsCommand(owner, command);
      if (!active()) return false;
      accept(result, owner);
      saved.value = true;
      return true;
    } catch (e) {
      if (active()) {
        error.value = failure(e).message;
        try {
          const result = await core.shortcutSettings(owner);
          if (active()) accept(result, owner);
        } catch {
          /* Explicit refresh is required. */
        }
      }
      return false;
    } finally {
      if (active()) busy.value = false;
    }
  }
  function change(edit: ShortcutEdit) {
    return canChange.value && view.value?.current
      ? command({ kind: 'change', expected_revision: view.value.current.revision, edit })
      : Promise.resolve(false);
  }
  function repair() {
    return view.value?.damaged ? command({ kind: 'repair' }) : Promise.resolve(false);
  }
  async function beginRecording(action: ShortcutAction, scope: ShortcutScope) {
    if (!canChange.value || !epoch || !view.value?.current) return;
    const binding = view.value.current.bindings[action];
    if (!binding) return;
    const owner = epoch,
      current = ++recordSerial;
    draft.value = {
      action,
      scope,
      id: null,
      keys: [...(binding[scope] ?? [])],
      revision: view.value.current.revision,
      changed: false,
    };
    recordError.value = '';
    saved.value = false;
    try {
      const id = await core.shortcutBeginRecording(owner);
      if (disposed || owner !== epoch || current !== recordSerial || !draft.value) {
        await core.shortcutEndRecording(id);
        return;
      }
      draft.value = { ...draft.value, id };
    } catch (e) {
      if (!disposed && owner === epoch && current === recordSerial) {
        draft.value = undefined;
        error.value = failure(e).message;
      }
    }
  }
  async function capture(input: KeyInput) {
    const captured = draft.value;
    if (!captured?.id || disposed || busy.value) return;
    const current = ++captureSerial;
    try {
      const result = await core.shortcutCapture(input, captured.scope);
      if (disposed || current !== captureSerial || draft.value?.id !== captured.id) return;
      if (result.kind === 'ignore') {
        recordError.value = '这组按键不能用于自定义快捷键';
        return;
      }
      draft.value = {
        ...draft.value,
        keys: result.kind === 'clear' ? [] : result.keys,
        changed: true,
      };
      recordError.value = '';
    } catch (e) {
      if (!disposed && current === captureSerial && draft.value?.id === captured.id)
        recordError.value = failure(e).message;
    }
  }
  async function saveRecording() {
    const captured = draft.value;
    if (!captured?.id || !captured.changed || busy.value) return false;
    const ok = await command({
      kind: 'change',
      expected_revision: captured.revision,
      edit: { kind: 'bind', action: captured.action, scope: captured.scope, keys: captured.keys },
    });
    if (
      draft.value?.id === captured.id &&
      (ok || view.value?.current?.revision !== captured.revision)
    )
      await cancelRecording();
    return ok;
  }
  function observeNative(status: ShortcutStatus | undefined) {
    if (
      draft.value?.id &&
      status &&
      status.phase !== 'starting' &&
      status.recording !== draft.value.id
    ) {
      void cancelRecording();
      error.value = '快捷键录入已结束，请重新选择要修改的按键';
    }
  }
  async function resumeRecording(id: string) {
    const owner = epoch;
    try {
      await core.shortcutEndRecording(id);
      if (!disposed && owner === epoch) await read();
    } catch (e) {
      if (!disposed && owner === epoch) error.value = failure(e).message;
    }
  }
  function dispose() {
    disposed = true;
    serial++;
    void cancelRecording();
  }
  return {
    view,
    draft,
    loading,
    busy,
    error,
    saved,
    recordError,
    canChange,
    connect,
    read,
    change,
    repair,
    beginRecording,
    capture,
    saveRecording,
    cancelRecording,
    resumeRecording,
    observeNative,
    dispose,
  };
}
