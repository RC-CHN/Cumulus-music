<script setup lang="ts">
import BoundedPanelTransition from './BoundedPanelTransition.vue';
import { computed, ref, shallowRef, watch } from 'vue';
import ActionMenu from './ActionMenu.vue';
import Icon from './Icon.vue';
import ModalTransition from './ModalTransition.vue';
import VoicePodcastUncollectDialog from './VoicePodcastUncollectDialog.vue';
import HistoryConfirm from './history/HistoryConfirm.vue';
import HistoryRadioMenu, { type HistoryRadioMenuTarget } from './history/HistoryRadioMenu.vue';
import { useVirtualRows } from '../composables/useVirtualRows';
import { usePodcastFollow } from '../composables/usePodcastFollow';
import type { HistoryModel } from '../composables/useHistory';
import { failure, type AccountView } from '../services/contracts';
import { useCore } from '../services/core';
import type { PodcastSearchFollow } from '../services/podcast';
import {
  historyLabel,
  historyOutcome,
  historyPending,
  historyTabs,
  historyTime,
  selectedHistoryBatch,
  type HistoryAction,
  type HistoryCollectTarget,
  type HistoryCommand,
  type HistoryEntry,
  type HistoryKind,
  type HistoryQueueAction,
  type HistoryQueueKind,
  type HistoryRadioKind,
  type HistoryRadioMenuAction,
  type HistoryResource,
  type HistorySelectionScope,
} from '../services/history';
import { normalizeKeyword } from '../services/keyword';
import type { MenuItem } from '../services/menu';
const props = defineProps<{
  model: HistoryModel;
  account: AccountView;
  playable: boolean;
  batchMode: boolean;
  collecting?: boolean;
  collectFeedback?: { message: string; failed?: boolean };
  batchSelection: (
    kind: HistoryQueueKind,
    revision: string,
    action: HistoryQueueAction,
  ) => Promise<boolean>;
  radioSelection: (
    kind: HistoryRadioKind,
    revision: string,
    id: string,
    action: HistoryRadioMenuAction,
  ) => Promise<void>;
  currentId?: string;
  currentVoiceId?: string;
}>();
const emit = defineEmits<{
  scope: [kind: HistoryKind];
  batch: [kind: HistoryQueueKind];
  leaveBatch: [];
  play: [kind: HistoryKind, revision: string, ids: string[], index: number];
  collectSongs: [target: HistoryCollectTarget, opener: HTMLElement];
  downloadVoices: [target: HistorySelectionScope];
  chapter: [kind: 'podcast' | 'audiobook', revision: string, id: string];
  open: [resource: HistoryResource];
  login: [];
  expired: [];
}>();
const { page, kind, loading, busy, error, uncertain, pending, canMutate } = props.model;
const keyword = ref('');
const selected = shallowRef(new Set<string>());
const entries = computed(() => {
  const needle = normalizeKeyword(keyword.value);
  return (page.value?.entries ?? []).filter((entry) => {
    const label = historyLabel(entry);
    return !needle || normalizeKeyword(`${label.name} ${label.subtitle}`).includes(needle);
  });
});
const { container, start, end, onScroll, reset } = useVirtualRows(() => entries.value.length, 68);
const allSelected = computed(
  () => entries.value.length > 0 && entries.value.every((e) => selected.value.has(e.id)),
);
const listKind = computed(() => kind.value === 'song' || kind.value === 'voice');
const selectRows = computed(() => !listKind.value || props.batchMode);
const headerItems = computed<MenuItem[]>(() => [
  { key: 'add-all', label: '添加全部至播放列表', icon: 'plus', disabled: !props.playable },
  { key: 'batch', label: '批量操作', icon: 'list' },
  { key: 'clear', label: '清空列表', disabled: !canMutate.value },
]);
const selectedBatch = computed(() =>
  page.value?.kind === kind.value
    ? selectedHistoryBatch(kind.value, entries.value, selected.value)
    : [],
);
const addNotice = ref('');
const batchError = ref('');
watch([kind, keyword, () => props.account.epoch], () => {
  addNotice.value = '';
  batchError.value = '';
});
const label = computed(() => historyTabs.find((t) => t.kind === kind.value)?.label ?? '记录');
const activeOperations = computed(() => page.value?.operations.filter(historyPending) ?? []);
const latest = computed(() => page.value?.operations.find((op) => !historyPending(op)));
const confirm = shallowRef<{ title: string; description: string; command: HistoryCommand }>();
const core = useCore();
const radioKind = computed(() =>
  kind.value === 'podcast' || kind.value === 'audiobook' ? kind.value : undefined,
);
const collectionScope = () =>
  page.value && page.value.kind === radioKind.value
    ? `${page.value.kind}:${page.value.revision}`
    : '';
const collection = usePodcastFollow(
  core,
  () => props.account,
  () => radioKind.value !== undefined,
  () => emit('expired'),
  {
    scope: collectionScope,
    receipt: collectionScope,
    success: (subscribed) => (subscribed ? '收藏成功' : '取消收藏成功'),
    action: (owner, id, state) => {
      const snapshot = page.value;
      const category = radioKind.value;
      if (
        !category ||
        snapshot?.kind !== category ||
        props.account.epoch !== owner ||
        loading.value ||
        busy.value ||
        !entries.value.some((row) => row.id === id && row.canCollectRadio)
      )
        return;
      return () =>
        core.historyRadioCollection(
          owner,
          category,
          snapshot.revision,
          id,
          state.kind === 'toggle'
            ? { kind: 'toggle', expected: state.subscribed }
            : { kind: 'reconcile' },
        );
    },
  },
);
const shown = computed(() =>
  entries.value.slice(start.value, end.value).map((entry) => ({
    entry,
    follow:
      entry.canCollectRadio && collectionScope()
        ? props.account.status === 'authenticated'
          ? collection.control(entry.id)
          : props.account.status !== 'unknown'
            ? ({ kind: 'toggle', subscribed: false, busy: false } as const)
            : undefined
        : undefined,
  })),
);
const historyPage = ref<HTMLElement>();
const uncollect = shallowRef<{
  id: string;
  scope: string;
  epoch: string;
  state: PodcastSearchFollow;
}>();
let uncollectOpener: HTMLElement | null = null;
const collectionFallback = () =>
  historyPage.value?.querySelector<HTMLElement>('[aria-current="page"]') ?? null;
watch([collectionScope, () => props.account.epoch], () => (uncollect.value = undefined), {
  flush: 'sync',
});
function collectionLabel(state: PodcastSearchFollow) {
  return state.kind === 'reconcile' ? '核对收藏' : state.subscribed ? '取消收藏' : '收藏';
}
function collect(id: string, state: PodcastSearchFollow, opener: HTMLElement) {
  if (loading.value || busy.value || state.busy || !collectionScope()) return;
  if (props.account.status !== 'authenticated') {
    emit('login');
    return;
  }
  if (state.kind === 'toggle' && state.subscribed) {
    uncollectOpener = opener;
    uncollect.value = { id, scope: collectionScope(), epoch: props.account.epoch, state };
  } else void collection.activate(id, state);
}
const radioMenu = ref<InstanceType<typeof HistoryRadioMenu>>();
function radioMenuLive(target: HistoryRadioMenuTarget) {
  return (
    target.epoch === props.account.epoch &&
    target.keyword === keyword.value &&
    target.kind === kind.value &&
    page.value?.kind === target.kind &&
    page.value.revision === target.revision &&
    shown.value.some(({ entry }) => entry.id === target.id)
  );
}
function rowCollection(id: string) {
  return shown.value.find((row) => row.entry.id === id)?.follow;
}
function canCollectFromMenu(target: HistoryRadioMenuTarget) {
  const follow = rowCollection(target.id);
  return !!follow && follow.kind === 'toggle' && !follow.subscribed && !follow.busy;
}
function showRadioMenu(entry: HistoryEntry, event: MouseEvent | KeyboardEvent) {
  const snapshot = page.value;
  if (
    !snapshot ||
    !radioKind.value ||
    snapshot.kind !== radioKind.value ||
    entry.resource.kind !== radioKind.value ||
    !(event.currentTarget instanceof HTMLElement)
  )
    return;
  const anchor = event.currentTarget
    .closest('.history-row')
    ?.querySelector<HTMLElement>('.history-radio-more');
  if (!anchor) return;
  event.preventDefault();
  event.stopPropagation();
  void radioMenu.value?.show(
    {
      kind: radioKind.value,
      revision: snapshot.revision,
      id: entry.id,
      name: entry.resource.radio.name,
      unavailable: entry.resource.radio.unavailable,
      epoch: props.account.epoch,
      keyword: keyword.value,
      anchor,
    },
    event instanceof MouseEvent && event.type === 'contextmenu'
      ? { x: event.clientX, y: event.clientY }
      : undefined,
  );
}
function radioMenuKeyboard(entry: HistoryEntry, event: KeyboardEvent) {
  if (event.key === 'ContextMenu' || (event.shiftKey && event.key === 'F10'))
    showRadioMenu(entry, event);
}
async function selectRadioMenu(
  action: 'open' | 'play' | 'next' | 'collect' | 'remove',
  target: HistoryRadioMenuTarget,
) {
  if (!radioMenuLive(target)) return;
  const entry = entries.value.find((row) => row.id === target.id);
  if (!entry) return;
  if (action === 'open') emit('open', entry.resource);
  else if (action === 'remove') remove({ kind: 'delete', ids: [entry.id] });
  else if (action === 'collect') {
    const follow = rowCollection(entry.id);
    if (follow?.kind === 'toggle' && !follow.subscribed) collect(entry.id, follow, target.anchor);
  } else {
    if (!props.playable || loading.value || busy.value || target.unavailable) return;
    batchError.value = '';
    try {
      await props.radioSelection(target.kind, target.revision, target.id, action);
    } catch (cause) {
      if (!radioMenuLive(target)) return;
      const issue = failure(cause);
      if (issue.code === 'profile_changed' || issue.code === 'session_expired') emit('expired');
      batchError.value = issue.message;
    }
  }
}
function confirmUncollect() {
  const saved = uncollect.value;
  uncollect.value = undefined;
  if (saved?.scope === collectionScope() && saved.epoch === props.account.epoch)
    void collection.activate(saved.id, saved.state);
}
watch(kind, () => {
  keyword.value = '';
  selected.value = new Set();
  reset();
});
watch(keyword, () => {
  selected.value = new Set();
  reset();
});
watch(
  () => page.value?.revision,
  () => {
    selected.value = new Set();
  },
);
function choose(id: string) {
  const next = new Set(selected.value);
  if (next.has(id)) next.delete(id);
  else next.add(id);
  selected.value = next;
}
function toggleAll() {
  selected.value = new Set(allSelected.value ? [] : entries.value.map((e) => e.id));
}
function playing(entry: HistoryEntry) {
  return entry.resource.kind === 'song'
    ? entry.resource.track.id === props.currentId
    : entry.resource.kind === 'voice' && entry.resource.voice.id === props.currentVoiceId;
}
function play(entry?: HistoryEntry) {
  if (!page.value || !props.playable || loading.value || busy.value) return;
  const playable = entries.value.filter((e) => historyLabel(e).available);
  const chosen = listKind.value ? playable : entry ? [entry] : [];
  const index = entry ? chosen.findIndex((e) => e.id === entry.id) : 0;
  if (index < 0 || !chosen[index] || !historyLabel(chosen[index]).available) return;
  emit(
    'play',
    kind.value,
    page.value.revision,
    chosen.map((e) => e.id),
    index,
  );
}
async function runQueueAction(action: HistoryQueueAction) {
  const snapshot = page.value;
  const all = action.kind === 'add-all';
  if (
    !snapshot ||
    (snapshot.kind !== 'song' && snapshot.kind !== 'voice') ||
    props.batchMode === all ||
    !props.playable ||
    loading.value ||
    busy.value ||
    !(all ? snapshot.entries.length : action.ids.length)
  )
    return;
  const epoch = props.account.epoch;
  const scope = keyword.value;
  const current = () =>
    epoch === props.account.epoch &&
    snapshot.kind === kind.value &&
    (all || scope === keyword.value);
  addNotice.value = '';
  batchError.value = '';
  try {
    if (
      (await props.batchSelection(snapshot.kind, snapshot.revision, action)) &&
      action.kind !== 'play' &&
      current()
    )
      addNotice.value = '已添加到播放列表';
  } catch (cause) {
    if (!current()) return;
    const issue = failure(cause);
    if (issue.code === 'profile_changed' || issue.code === 'session_expired') emit('expired');
    batchError.value = issue.message;
  }
}
function downloadSelectedVoices() {
  const snapshot = page.value;
  const ids = selectedBatch.value;
  if (
    !props.batchMode ||
    kind.value !== 'voice' ||
    snapshot?.kind !== 'voice' ||
    loading.value ||
    busy.value ||
    !ids.length
  )
    return;
  emit('downloadVoices', { revision: snapshot.revision, ids: [...ids] });
}
function collectSelectedSongs(event: MouseEvent) {
  const snapshot = page.value;
  const ids = selectedBatch.value;
  if (
    !props.batchMode ||
    props.collecting ||
    kind.value !== 'song' ||
    snapshot?.kind !== 'song' ||
    loading.value ||
    busy.value ||
    !ids.length ||
    !(event.currentTarget instanceof HTMLElement)
  )
    return;
  emit(
    'collectSongs',
    { kind: 'selected', revision: snapshot.revision, ids: [...ids] },
    event.currentTarget,
  );
}
function collectAllSongs(event: MouseEvent) {
  const snapshot = page.value;
  if (
    props.batchMode ||
    props.collecting ||
    kind.value !== 'song' ||
    snapshot?.kind !== 'song' ||
    !snapshot.entries.length ||
    loading.value ||
    busy.value ||
    !(event.currentTarget instanceof HTMLElement)
  )
    return;
  emit('collectSongs', { kind: 'all', revision: snapshot.revision }, event.currentTarget);
}
function chooseHeader(key: string) {
  const snapshot = page.value;
  if (
    !snapshot?.entries.length ||
    snapshot.kind !== kind.value ||
    (snapshot.kind !== 'song' && snapshot.kind !== 'voice') ||
    props.batchMode ||
    loading.value ||
    busy.value
  )
    return;
  if (key === 'add-all') void runQueueAction({ kind: 'add-all' });
  else if (key === 'batch') emit('batch', snapshot.kind);
  else if (key === 'clear') remove({ kind: 'clear' });
}
function playChapter(entry: HistoryEntry) {
  const snapshot = page.value;
  if (
    !snapshot?.chapterRevision ||
    !props.playable ||
    loading.value ||
    busy.value ||
    !entry.chapter?.playable ||
    (snapshot.kind !== 'podcast' && snapshot.kind !== 'audiobook') ||
    snapshot.kind !== kind.value
  )
    return;
  emit('chapter', snapshot.kind, snapshot.chapterRevision, entry.id);
}
function remove(action: HistoryAction) {
  if (!page.value || !canMutate.value) return;
  const count = action.kind === 'clear' ? page.value.entries.length : action.ids.length;
  if (!count) return;
  confirm.value = {
    title: action.kind === 'clear' ? `清空${label.value}播放记录？` : `删除 ${count} 条播放记录？`,
    description: `${action.kind === 'clear' ? '将清空这一类型的全部历史，不仅是筛选结果。' : '将移除选中的历史记录。'}${page.value.canSync && page.value.syncEnabled ? '同时请求删除当前账号的多端记录。' : '仅删除本机记录。'}音乐文件、收藏和歌单内容会保留。`,
    command: {
      kind: 'mutate',
      id: crypto.randomUUID(),
      expected_revision: page.value.revision,
      action,
    },
  };
}
function accept(id: string) {
  if (!page.value) return;
  confirm.value = {
    title: '保留当前读取的记录？',
    description:
      '这不会重发删除请求，也不代表原请求已经成功。原请求可能仍在服务端处理中；保留当前状态后，可继续管理记录。',
    command: { kind: 'accept_current', id, expected_revision: page.value.revision },
  };
}
async function submit() {
  if (!confirm.value) return;
  const input = confirm.value.command;
  const ok = await props.model.command(input);
  // A lost reply is represented in the page journal, not a tempting second
  // submit button. Native intent IDs are never regenerated on retry.
  if (ok || pending.value) confirm.value = undefined;
}
function keyboard(event: KeyboardEvent) {
  if (
    event.repeat ||
    !['Delete', 'Backspace'].includes(event.key) ||
    !(event.target instanceof HTMLElement) ||
    event.target.closest('input, textarea, select, [contenteditable], dialog')
  )
    return;
  if (selected.value.size && canMutate.value) {
    event.preventDefault();
    remove({ kind: 'delete', ids: [...selected.value] });
  }
}
</script>
<template>
  <section
    ref="historyPage"
    class="history-page"
    aria-label="最近播放"
    :data-history-category="label"
    :data-history-view="batchMode ? 'batch' : 'browse'"
    :aria-busy="loading"
    @keydown="keyboard"
  >
    <header class="history-heading">
      <div>
        <h1>{{ batchMode ? '批量操作' : '最近播放' }}</h1>
        <p class="muted">{{ batchMode ? `${label}播放记录` : '找回听过的音乐和声音' }}</p>
      </div>
      <button
        v-if="!batchMode"
        class="icon-button"
        aria-label="刷新播放记录"
        :disabled="loading || busy"
        @click="model.refresh"
      >
        <Icon name="refresh" />
      </button>
      <button v-else type="button" class="secondary" :disabled="busy" @click="emit('leaveBatch')">
        <Icon name="back" />退出批量操作
      </button>
    </header>
    <nav v-if="!batchMode" class="catalog-tabs" aria-label="播放记录类型">
      <button
        v-for="tab in historyTabs"
        :key="tab.kind"
        :aria-current="kind === tab.kind ? 'page' : undefined"
        :disabled="busy"
        @click="emit('scope', tab.kind)"
      >
        {{ tab.label }}
      </button>
    </nav>
    <div class="history-tools">
      <button
        v-if="listKind && !batchMode"
        class="primary"
        :disabled="!playable || loading || busy || !entries.some((e) => historyLabel(e).available)"
        @click="play()"
      >
        <Icon name="play" />播放全部
      </button>
      <button
        v-if="kind === 'song' && !batchMode"
        type="button"
        class="secondary history-collect-all"
        :disabled="loading || busy || !page?.entries.length"
        :aria-disabled="collecting"
        @click="collectAllSongs"
      >
        <Icon name="collect" />收藏全部
      </button>
      <ActionMenu
        v-if="listKind && !batchMode"
        data-history-menu
        label="更多播放记录操作"
        :items="headerItems"
        :disabled="loading || busy || !page?.entries.length"
        @select="chooseHeader"
      />
      <button
        v-if="batchMode"
        class="secondary history-play-selected"
        :disabled="!playable || loading || busy || !selectedBatch.length"
        @click="runQueueAction({ kind: 'play', ids: selectedBatch })"
      >
        <Icon name="play" />播放选中
      </button>
      <button
        v-if="batchMode"
        type="button"
        class="secondary history-add-selected"
        :disabled="!playable || loading || busy || !selectedBatch.length"
        @click="runQueueAction({ kind: 'add', ids: selectedBatch })"
      >
        <Icon name="plus" />添加到播放列表
      </button>
      <button
        v-if="batchMode && kind === 'song'"
        type="button"
        class="secondary history-collect-selected"
        :disabled="loading || busy || !selectedBatch.length"
        :aria-disabled="collecting"
        @click="collectSelectedSongs"
      >
        <Icon name="collect" />收藏
      </button>
      <button
        v-if="batchMode && kind === 'voice'"
        type="button"
        class="secondary history-download-selected"
        :disabled="loading || busy || !selectedBatch.length"
        @click="downloadSelectedVoices"
      >
        <Icon name="download" />下载
      </button>
      <label class="history-search"
        ><Icon name="search" /><input
          v-model="keyword"
          aria-label="筛选播放记录"
          placeholder="搜索名称或创作者" /><button
          v-if="keyword"
          class="icon-button"
          aria-label="清空历史筛选"
          @click="keyword = ''"
        >
          <Icon name="close" /></button
      ></label>
      <button
        v-if="selectRows"
        class="secondary"
        :disabled="!canMutate || !selected.size"
        @click="remove({ kind: 'delete', ids: [...selected] })"
      >
        删除选中<span v-if="selected.size">（{{ selected.size }}）</span>
      </button>
      <button
        v-if="!listKind"
        class="text-button"
        :disabled="!canMutate || !page?.entries.length"
        @click="remove({ kind: 'clear' })"
      >
        清空列表
      </button>
    </div>
    <div class="history-status">
      <span aria-live="polite"
        >{{ entries.length }} 条<template v-if="keyword">匹配记录</template
        ><template v-if="loading"> · 正在读取…</template></span
      ><label :title="page?.canSync ? '合并本机和当前账号的多端记录' : '登录后可查看多端记录'"
        ><input
          type="checkbox"
          :checked="page?.syncEnabled && page?.canSync"
          :disabled="!page?.canSync || loading || busy || pending"
          @change="model.setSync(($event.target as HTMLInputElement).checked)"
        />多端记录</label
      >
    </div>
    <p v-if="collection.status.value" class="history-outcome" role="status">
      {{ collection.status.value }}
    </p>
    <button
      v-if="
        collectionScope() &&
        account.status === 'authenticated' &&
        collection.membershipUnknown.value &&
        entries.some((entry) => entry.canCollectRadio)
      "
      class="text-button history-collection-retry"
      type="button"
      @click="collection.retry"
    >
      重新读取收藏
    </button>
    <div
      v-if="
        collectFeedback ||
        addNotice ||
        batchError ||
        error ||
        uncertain ||
        activeOperations.length ||
        latest
      "
      class="history-notices"
    >
      <p
        v-if="collectFeedback"
        :class="collectFeedback.failed ? 'catalog-error' : 'history-outcome'"
        :role="collectFeedback.failed ? 'alert' : 'status'"
        class="history-collect-feedback"
      >
        {{ collectFeedback.message }}
      </p>
      <p v-if="addNotice" class="history-outcome history-add-notice" role="status">
        {{ addNotice }}
      </p>
      <p v-if="batchError" class="catalog-error history-add-error" role="alert">{{ batchError }}</p>
      <div v-if="error" class="catalog-error" role="alert">
        {{ error }}<span v-if="page"> · 保留上次读取的内容</span
        ><button class="text-button" :disabled="loading || busy" @click="model.refresh">
          重新读取
        </button>
      </div>
      <div v-if="uncertain" class="history-recovery" role="status">
        <p>
          未收到操作结果。先读取本机操作记录，或取消尚未开始的请求；已经开始的请求会保留真实状态。
        </p>
        <button class="secondary" :disabled="busy || loading" @click="model.localRefresh">
          读取操作记录</button
        ><button class="secondary" :disabled="busy || loading" @click="model.cancelUnknown">
          取消未开始的请求
        </button>
      </div>
      <div v-for="op in activeOperations" :key="op.id" class="history-recovery" role="status">
        <p>{{ historyOutcome(op) }}；本机已移除 {{ op.localRemoved }} 条。</p>
        <button
          class="secondary"
          :disabled="busy || loading || !page?.syncEnabled"
          @click="model.command({ kind: 'reconcile', id: op.id })"
        >
          核对多端记录</button
        ><button
          v-if="op.lastReadAt"
          class="secondary"
          :disabled="busy || loading"
          @click="accept(op.id)"
        >
          保留当前状态
        </button>
      </div>
      <p v-if="latest" class="history-outcome" role="status">{{ historyOutcome(latest) }}</p>
    </div>
    <BoundedPanelTransition name="catalog-tab">
      <div :key="kind" class="history-content">
        <div
          v-if="entries.length"
          class="history-table"
          :class="{ 'history-table-browse': !selectRows }"
          role="table"
          :aria-label="`${label}播放记录`"
          :aria-rowcount="entries.length + 1"
          :aria-colcount="selectRows ? 5 : 4"
        >
          <div class="history-row history-table-heading" role="row">
            <div v-if="selectRows" role="columnheader">
              <input
                type="checkbox"
                aria-label="选择筛选后的全部记录"
                :checked="allSelected"
                :indeterminate="selected.size > 0 && !allSelected"
                @change="toggleAll"
              />
            </div>
            <span role="columnheader">{{ label }} / 创作者</span
            ><span role="columnheader">播放时间</span><span role="columnheader">设备</span
            ><span role="columnheader">操作</span>
          </div>
          <div ref="container" class="history-scroll" role="rowgroup" @scroll.passive="onScroll">
            <div class="history-spacer" :style="{ height: `${entries.length * 68}px` }">
              <div class="history-window" :style="{ transform: `translateY(${start * 68}px)` }">
                <div
                  v-for="({ entry, follow }, offset) in shown"
                  :key="entry.id"
                  class="history-row"
                  :class="{
                    'is-playing': playing(entry),
                    'is-selected': selected.has(entry.id),
                    'is-radio': entry.resource.kind === radioKind,
                  }"
                  role="row"
                  :aria-rowindex="start + offset + 2"
                  @dblclick="play(entry)"
                  @contextmenu="showRadioMenu(entry, $event)"
                  @keydown="radioMenuKeyboard(entry, $event)"
                >
                  <div v-if="selectRows" role="cell" @dblclick.stop>
                    <input
                      type="checkbox"
                      :aria-label="`选择 ${historyLabel(entry).name}`"
                      :checked="selected.has(entry.id)"
                      @change="choose(entry.id)"
                    />
                  </div>
                  <div class="history-name-cell" role="cell">
                    <img
                      v-if="historyLabel(entry).cover"
                      :src="historyLabel(entry).cover!"
                      alt=""
                      loading="lazy"
                      decoding="async"
                    /><span v-else class="history-cover"><Icon name="music" /></span>
                    <div>
                      <button
                        class="history-name"
                        :title="historyLabel(entry).name"
                        @click="emit('open', entry.resource)"
                      >
                        {{ historyLabel(entry).name || '未命名' }}
                      </button>
                      <div v-if="entry.chapter" class="history-chapter" @dblclick.stop>
                        <span v-if="entry.chapter.recent" class="history-chapter-prefix"
                          >最近播放：</span
                        >
                        <span v-else-if="entry.chapter.tag" class="history-chapter-prefix"
                          >{{ entry.chapter.tag }}：</span
                        >
                        <button
                          v-if="entry.chapter.playable"
                          type="button"
                          :disabled="!playable || loading || busy"
                          :title="entry.chapter.name"
                          :aria-label="`播放章节 ${entry.chapter.name}`"
                          @click.stop="playChapter(entry)"
                        >
                          {{ entry.chapter.name }}
                        </button>
                        <span v-else class="history-chapter-name" :title="entry.chapter.name">
                          {{ entry.chapter.name }}
                        </span>
                        <span
                          v-if="entry.chapter.completed || entry.chapter.percent !== null"
                          class="history-chapter-progress"
                        >
                          {{
                            entry.chapter.completed ? '已播完' : `已播 ${entry.chapter.percent}%`
                          }}
                        </span>
                      </div>
                      <p v-else :title="historyLabel(entry).subtitle">
                        {{ historyLabel(entry).subtitle || '未知创作者'
                        }}<span v-if="!historyLabel(entry).available"> · 暂不可播放</span>
                      </p>
                    </div>
                    <div
                      v-if="entry.resource.kind === radioKind"
                      class="history-radio-row-controls"
                      @dblclick.stop
                    >
                      <button
                        v-if="follow"
                        class="icon-button history-radio-collect"
                        type="button"
                        :data-history-radio-collect="entry.id"
                        :title="collectionLabel(follow)"
                        :aria-label="`${collectionLabel(follow)} ${historyLabel(entry).name}`"
                        :aria-pressed="follow.kind === 'toggle' ? follow.subscribed : undefined"
                        :aria-disabled="loading || busy || follow.busy"
                        @click.stop="collect(entry.id, follow, $event.currentTarget as HTMLElement)"
                      >
                        <Icon
                          :name="
                            follow.kind === 'reconcile'
                              ? 'refresh'
                              : follow.subscribed
                                ? 'check'
                                : 'collect'
                          "
                        />
                      </button>
                      <button
                        type="button"
                        class="icon-button history-radio-more"
                        :data-history-radio-more="entry.id"
                        :aria-label="`更多：${historyLabel(entry).name}`"
                        title="更多"
                        aria-haspopup="menu"
                        aria-expanded="false"
                        @click.stop="showRadioMenu(entry, $event)"
                        @keydown.down.prevent="showRadioMenu(entry, $event)"
                      >
                        <Icon name="more" />
                      </button>
                    </div>
                  </div>
                  <div class="history-time" role="cell" :title="historyTime(entry.playedAt).full">
                    <span>{{ historyTime(entry.playedAt).short }}</span
                    ><small v-if="entry.playProgress !== null">{{
                      entry.listened
                        ? '已听完'
                        : `已听 ${Math.floor(entry.playProgress / 60)} 分 ${Math.floor(entry.playProgress % 60)} 秒`
                    }}</small>
                  </div>
                  <span class="history-device" role="cell">{{ entry.device }}</span>
                  <div class="history-actions" role="cell" @dblclick.stop>
                    <button
                      class="icon-button"
                      :aria-label="`播放 ${historyLabel(entry).name}`"
                      :disabled="!playable || loading || busy || !historyLabel(entry).available"
                      @click.stop="play(entry)"
                    >
                      <Icon name="play" /></button
                    ><button
                      class="icon-button"
                      :aria-label="`删除 ${historyLabel(entry).name} 的播放记录`"
                      :disabled="!canMutate"
                      @click.stop="remove({ kind: 'delete', ids: [entry.id] })"
                    >
                      <Icon name="close" />
                    </button>
                  </div>
                </div>
              </div>
            </div>
          </div>
        </div>
        <p v-else-if="!loading && page" class="empty-state">
          {{
            keyword
              ? '没有匹配的播放记录'
              : error
                ? '当前没有可显示的缓存记录'
                : '这里还没有播放记录'
          }}
        </p>
      </div>
    </BoundedPanelTransition>
    <HistoryRadioMenu
      ref="radioMenu"
      :signed-in="account.status === 'authenticated'"
      :can-play="playable && !loading && !busy"
      :can-remove="canMutate"
      :can-collect="canCollectFromMenu"
      :live="radioMenuLive"
      :fallback-focus="collectionFallback"
      @choose="selectRadioMenu"
      @login="emit('login')"
    />
    <Teleport to="body">
      <ModalTransition :return-focus="() => uncollectOpener" :fallback-focus="collectionFallback">
        <VoicePodcastUncollectDialog
          v-if="uncollect"
          @close="uncollect = undefined"
          @confirm="confirmUncollect"
        />
      </ModalTransition>
    </Teleport>
    <p class="history-note muted">
      {{ page?.remoteIncluded ? '本机与多端记录已合并，按最近播放排序' : '显示本机播放记录'
      }}<template v-if="page?.remoteIncluded && page.remoteFetchedAt">
        · 最近读取 {{ historyTime(page.remoteFetchedAt).short }}</template
      >
    </p>
    <ModalTransition
      ><HistoryConfirm
        v-if="confirm"
        :title="confirm.title"
        :description="confirm.description"
        :busy="busy"
        :error="error"
        @close="confirm = undefined"
        @confirm="submit"
    /></ModalTransition>
  </section>
</template>
