import { computed, onMounted, onUnmounted, ref, watch } from 'vue';
import type { PlaylistManager } from './usePlaylists';
import type { AccountView } from '../services/contracts';
import { playlistLength, playlistPending } from '../services/playlists.ts';
import { rebasePlaylistDraft, type PlaylistDraft, type PlaylistField } from './playlistDraft.ts';
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
  const baseline = ref<PlaylistDraft>({ name: '', description: '', tags: [] });
  let baselineRevision = '';
  const needsRefresh = ref(false);
  const refreshing = ref(false);
  const conflicts = ref<PlaylistField[]>([]);
  const refreshBlocked = computed(
    () => needsRefresh.value || refreshing.value || conflicts.value.length > 0,
  );
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
    baselineRevision = editor.revision;
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
  function applyDraft(draft: PlaylistDraft) {
    name.value = draft.name;
    description.value = draft.description;
    selectedTags.value = [...draft.tags];
  }
  function rememberConflict() {
    if (manager.issue.value?.code === 'snapshot_changed') {
      needsRefresh.value = true;
      message.value = '';
    }
  }
  async function refreshDraft() {
    if (!active() || !needsRefresh.value || refreshing.value || pending.value || manager.busy.value)
      return;
    refreshing.value = true;
    try {
      const latest = await manager.refreshEditor(owner, id);
      if (!active() || !latest) return;
      const next = rebasePlaylistDraft(
        baseline.value,
        { name: name.value, description: description.value, tags: selectedTags.value },
        latest,
      );
      baseline.value = {
        name: latest.name,
        description: latest.description,
        tags: [...latest.tags],
      };
      baselineRevision = latest.revision;
      applyDraft(next.draft);
      conflicts.value = next.conflicts;
      needsRefresh.value = false;
      message.value = '已读取最新歌单，草稿中的修改已保留。';
    } finally {
      if (active()) refreshing.value = false;
    }
  }
  function resolveConflicts(keepDraft: boolean) {
    if (!active() || !conflicts.value.length) return;
    if (!keepDraft) {
      if (conflicts.value.includes('name')) name.value = baseline.value.name;
      if (conflicts.value.includes('description')) description.value = baseline.value.description;
      if (conflicts.value.includes('tags')) selectedTags.value = [...baseline.value.tags];
    }
    conflicts.value = [];
  }
  function requestLeave(next = close) {
    if (!active()) return;
    if (manager.busy.value || refreshing.value) {
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
      refreshBlocked.value ||
      manager.busy.value
    )
      return;
    const result = await manager.mutate({
      kind: 'metadata',
      id,
      revision: baselineRevision,
      name: name.value.trim(),
      description: description.value,
      tags: [...selectedTags.value],
    });
    if (!active()) return;
    if (result && manager.saved(result.phase)) close();
    else if (!result) rememberConflict();
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
    if (!p || !editable.value || pending.value || refreshBlocked.value) return;
    const result = await manager.mutate(
      kind === 'clear'
        ? { kind: 'tracks', id, revision: baselineRevision, change: 'clear', ids: [] }
        : { kind, id, revision: baselineRevision },
    );
    if (!active()) return;
    confirmation.value = undefined;
    if (result && manager.saved(result.phase)) close();
    else if (!result) rememberConflict();
  }
  async function coverSaved(outcome: CoverOutcome) {
    await manager.refreshAfterCover(owner, id, outcome.editor);
    if (active()) {
      message.value = '封面已保存。';
      coverChanged();
    }
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
    refreshDraft,
    needsRefresh,
    refreshing,
    refreshBlocked,
    conflicts,
    latestDraft: baseline,
    resolveConflicts,
    save,
    reconcile,
    confirm,
    coverSaved,
  };
}
