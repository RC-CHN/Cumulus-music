import { computed, onMounted, onUnmounted, ref, watch } from 'vue';
import type { PlaylistManager } from './usePlaylists';
import type { AccountView } from '../services/contracts';
import { playlistLength, playlistPending } from '../services/playlists';
import type { CoverOutcome } from '../services/playlistCover';

/** Form draft stays separate from the core's latest observed revision and cover. */
export function usePlaylistEditor(
  manager: PlaylistManager,
  account: () => AccountView,
  id: string,
  close: () => void,
  coverChanged: () => void,
) {
  const name = ref(''),
    description = ref(''),
    selectedTags = ref<string[]>([]);
  const loaded = ref(false),
    message = ref('');
  const baseline = ref({ name: '', description: '', tags: [] as string[] });
  const confirmation = ref<'leave' | 'delete' | 'clear' | 'publish'>();
  let leave: (() => void) | undefined;
  let alive = true;
  const owner = account().epoch;
  const active = () => alive && account().epoch === owner;
  const outcome = computed(() =>
    manager.result.value?.playlistId === id
      ? manager.result.value
      : manager.operations.value.find((op) => op.playlistId === id && playlistPending(op.phase)),
  );
  const pending = computed(() => !!outcome.value && playlistPending(outcome.value.phase));
  const editable = computed(
    () =>
      manager.editor.value?.id === id &&
      manager.editor.value.owned &&
      !manager.editor.value.favorite,
  );
  const fieldStatus = computed(() => {
    const observed = outcome.value?.observed;
    return {
      name: !observed ? '待核对' : observed.name.trim() === name.value.trim() ? '已保存' : '未保存',
      tags: !observed
        ? '待核对'
        : String(observed.tags) === String(selectedTags.value)
          ? '已保存'
          : '未保存',
      description: !observed
        ? '待核对'
        : observed.description === description.value
          ? '已保存'
          : '未保存',
    };
  });
  const invalid = computed(
    () =>
      !name.value.trim() ||
      /[@#]/.test(name.value) ||
      playlistLength(name.value.trim()) > 40 ||
      playlistLength(description.value) > 1000,
  );
  const dirty = computed(
    () =>
      loaded.value &&
      (name.value.trim() !== baseline.value.name.trim() ||
        description.value !== baseline.value.description ||
        String(selectedTags.value) !== String(baseline.value.tags)),
  );
  watch(manager.editor, (editor) => {
    if (!active() || !editor || editor.id !== id || loaded.value) return;
    name.value = editor.name;
    description.value = editor.description;
    selectedTags.value = [...editor.tags];
    baseline.value = { name: editor.name, description: editor.description, tags: [...editor.tags] };
    loaded.value = true;
  });
  onMounted(() => {
    manager.reset();
    void manager.open(id);
    void manager.loadTags();
  });
  onUnmounted(() => {
    alive = false;
    leave = undefined;
  });
  function requestLeave(next = close) {
    if (!active()) return;
    if (manager.busy.value) {
      message.value = '正在保存并核对，请稍候再离开。';
      return;
    }
    if (dirty.value || pending.value) {
      leave = next;
      confirmation.value = 'leave';
    } else next();
  }
  async function save() {
    if (!active()) return;
    const p = manager.editor.value;
    if (
      !p ||
      !editable.value ||
      invalid.value ||
      !dirty.value ||
      pending.value ||
      manager.busy.value
    )
      return;
    const result = await manager.mutate({
      kind: 'metadata',
      id,
      revision: p.revision,
      name: name.value.trim(),
      description: description.value,
      tags: [...selectedTags.value],
    });
    if (active() && result && manager.saved(result.phase)) close();
  }
  async function reconcile() {
    if (!active()) return;
    const op = outcome.value;
    if (!op || manager.busy.value) return;
    const result = await manager.reconcile(op.operationId);
    if (active() && result && manager.saved(result.phase)) close();
  }
  async function confirm() {
    if (!active()) return;
    const kind = confirmation.value;
    if (!kind || manager.busy.value) return;
    if (kind === 'leave') {
      const next = leave;
      leave = undefined;
      confirmation.value = undefined;
      next?.();
      return;
    }
    const p = manager.editor.value;
    if (!p || !editable.value || pending.value) return;
    const result = await manager.mutate(
      kind === 'clear'
        ? { kind: 'tracks', id, revision: p.revision, change: 'clear', ids: [] }
        : { kind, id, revision: p.revision },
    );
    if (!active()) return;
    confirmation.value = undefined;
    if (result && manager.saved(result.phase)) close();
  }
  async function coverSaved(outcome: CoverOutcome) {
    await manager.refreshAfterCover(owner, id, outcome.editor);
    if (active()) coverChanged();
  }
  return {
    name,
    description,
    selectedTags,
    loaded,
    message,
    confirmation,
    pending,
    outcome,
    fieldStatus,
    editable,
    invalid,
    dirty,
    requestLeave,
    save,
    reconcile,
    confirm,
    coverSaved,
  };
}
