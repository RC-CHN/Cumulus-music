import { computed, ref, shallowRef } from 'vue';
import { failure } from '../services/contracts.ts';
import type {
  LyricEdit,
  LyricPreferencesCommand,
  LyricSettingsClient,
  LyricSettingsView,
} from '../services/desktopLyrics.ts';
export function useDesktopLyricSettings(client: LyricSettingsClient) {
  const view = shallowRef<LyricSettingsView>(),
    busy = ref(false),
    loading = ref(false),
    error = ref(''),
    saved = ref(false);
  let epoch = '',
    serial = 0,
    disposed = false;
  const preferences = computed(() => view.value?.preferences.current);
  const canChange = computed(() => !!preferences.value && !busy.value && !loading.value);
  let polling = false;
  async function status() {
    if (disposed || !epoch || busy.value || loading.value || polling) return;
    polling = true;
    const owner = epoch,
      current = serial;
    try {
      const value = await client.read(owner);
      if (!disposed && current === serial && owner === epoch) accept(value, owner);
    } catch {
      /* Explicit refresh reports failures without overwriting an edit error. */
    } finally {
      polling = false;
    }
  }
  function accept(value: LyricSettingsView, owner: string) {
    if (value.epoch !== owner)
      throw { code: 'stale', message: '账号已变化，请重新读取桌面歌词设置' };
    view.value = value;
  }
  async function read() {
    if (disposed || !epoch || busy.value) return;
    const owner = epoch,
      current = ++serial;
    loading.value = true;
    error.value = '';
    saved.value = false;
    try {
      const value = await client.read(owner);
      if (!disposed && serial === current) accept(value, owner);
    } catch (e) {
      if (!disposed && serial === current) error.value = failure(e).message;
    } finally {
      if (!disposed && serial === current) loading.value = false;
    }
  }
  async function connect(owner: string) {
    if (disposed || epoch === owner) return;
    epoch = owner;
    ++serial;
    view.value = undefined;
    busy.value = false;
    loading.value = false;
    error.value = '';
    await read();
  }
  async function send(command: LyricPreferencesCommand | 'retry' | 'unlock') {
    if (!epoch || disposed || busy.value || loading.value) return false;
    const owner = epoch,
      current = ++serial;
    const active = () => !disposed && current === serial && owner === epoch;
    busy.value = true;
    error.value = '';
    saved.value = false;
    try {
      const value =
        typeof command === 'string'
          ? await client.window(owner, command)
          : await client.change(owner, command);
      if (!active()) return false;
      accept(value, owner);
      saved.value = true;
      return true;
    } catch (e) {
      if (active()) {
        error.value = failure(e).message;
        // A lost acknowledgement is followed by a read, never a repeated write.
        try {
          const value = await client.read(owner);
          if (active()) accept(value, owner);
        } catch {
          /* Explicit refresh remains available. */
        }
      }
      return false;
    } finally {
      if (active()) busy.value = false;
    }
  }
  function change(edit: LyricEdit) {
    return preferences.value && canChange.value
      ? send({ kind: 'change', expected_revision: preferences.value.revision, edit })
      : Promise.resolve(false);
  }
  function dispose() {
    disposed = true;
    ++serial;
  }
  return {
    view,
    preferences,
    busy,
    loading,
    error,
    saved,
    canChange,
    connect,
    read,
    status,
    send,
    change,
    dispose,
  };
}
