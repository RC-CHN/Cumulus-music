import { computed, ref, shallowRef } from 'vue';
import { failure } from '../services/contracts.ts';
import type { LocalDirectoryClient, LocalDirectorySelection } from '../services/local.ts';

export function useLocalDirectories(core: LocalDirectoryClient, current = () => true) {
  const snapshot = shallowRef<LocalDirectorySelection>();
  const draft = shallowRef(new Map<string, boolean>());
  const busy = ref<'read' | 'pick' | 'apply'>();
  const error = ref('');
  const stale = ref(false);
  let alive = true;
  const live = () => alive && current();
  const usable = computed(() => !!snapshot.value && !busy.value && !stale.value);
  function install(value: LocalDirectorySelection, keepDraft: boolean) {
    const previous = draft.value;
    snapshot.value = value;
    draft.value = new Map(
      value.entries.map(({ id, checked }) => [
        id,
        (keepDraft ? previous.get(id) : undefined) ?? checked,
      ]),
    );
    stale.value = false;
  }
  async function read() {
    if (!live() || busy.value) return;
    busy.value = 'read';
    error.value = '';
    try {
      const value = await core.localDirectorySelection();
      if (live()) install(value, false);
    } catch (cause) {
      if (live()) error.value = failure(cause).message;
    } finally {
      if (live()) busy.value = undefined;
    }
  }
  function change(id: string, checked: boolean) {
    if (live() && usable.value && draft.value.has(id))
      draft.value = new Map(draft.value).set(id, checked);
  }
  function problem(cause: unknown) {
    const issue = failure(cause);
    stale.value = issue.code === 'snapshot_changed';
    error.value = stale.value ? '目录已在其他操作中改变，请重新读取后再选择。' : issue.message;
  }
  async function add() {
    const previous = snapshot.value;
    if (!live() || !usable.value || !previous) return;
    busy.value = 'pick';
    error.value = '';
    try {
      const value = await core.stageLocalDirectory(previous.revision);
      if (live() && value) install(value, true);
    } catch (cause) {
      if (live()) problem(cause);
    } finally {
      if (live()) busy.value = undefined;
    }
  }
  async function confirm() {
    const previous = snapshot.value;
    if (!live() || !usable.value || !previous) return;
    const choices = previous.entries.map(({ id, checked }) => ({
      id,
      checked: draft.value.get(id) ?? checked,
    }));
    busy.value = 'apply';
    error.value = '';
    try {
      const value = await core.applyLocalDirectories(previous.revision, choices);
      if (!live()) return;
      install(value, false);
      return { changed: value.revision !== previous.revision };
    } catch (cause) {
      if (live()) problem(cause);
    } finally {
      if (live()) busy.value = undefined;
    }
  }
  return {
    snapshot,
    draft,
    busy,
    error,
    stale,
    usable,
    read,
    change,
    add,
    confirm,
    dispose: () => {
      alive = false;
    },
  };
}
