<script setup lang="ts">
import { ref, computed, nextTick, onMounted, shallowRef, useId, watch } from 'vue';
import Icon from './Icon.vue';
import ActionMenu from './ActionMenu.vue';
import { failure } from '../services/contracts';
import LocalMatchDialog from './LocalMatchDialog.vue';
import LocalDirectoriesDialog from './LocalDirectoriesDialog.vue';
import ModalTransition from './ModalTransition.vue';
import LocalFolderGroups from './LocalFolderGroups.vue';
import type { LocalMatchingModel } from '../composables/useLocalMatching';
import type { LocalModel } from '../composables/useLocal';
import { useLocalGroupWidth } from '../composables/useLocalGroupWidth';
import { useLocalSearch } from '../composables/useLocalSearch';
import {
  localGroupQuery,
  localQueryKey,
  type LocalGroup,
  type LocalListCheckpoint,
  type LocalQuery,
  type LocalSort,
  type LocalTrack,
  type LocalRoot,
} from '../services/local';
import { time, type LocalQueueDropSource } from '../services/player';
const props = defineProps<{
  model: LocalModel;
  matching: LocalMatchingModel;
  playable: boolean;
  queueable: boolean;
  queueDropEnabled?: boolean;
  queueLocal: (query: LocalQuery, revision: string, trackId?: string) => Promise<unknown>;
  currentId?: string;
  batchReturn?: { checkpoint: LocalListCheckpoint; focusFrom: Element | null };
}>();
const emit = defineEmits<{
  play: [query: LocalQuery, revision: string, id: string | null];
  comments: [id: string, title: string];
  queueDrag: [event: PointerEvent, source: LocalQueueDropSource];
  batch: [query: LocalQuery, checkpoint: LocalListCheckpoint];
}>();
const { view, query, loading, busy, error, readError, notice } = props.model;
const groupCollator = new Intl.Collator('zh-CN-u-co-pinyin');
const namedGroups = computed(() => {
  const groups = view.value?.groups ?? [];
  return [...groups].sort((a, b) => {
    if (!a.key) return b.key ? 1 : 0;
    if (!b.key) return -1;
    return groupCollator.compare(a.name, b.name);
  });
});
const search = useLocalSearch(props.model);
const { keyword } = search;
const searchInput = ref<HTMLInputElement>();
const pageRoot = ref<HTMLElement>();
const browser = ref<HTMLElement>();
const groupList = ref<HTMLElement>();
const groupSeparator = ref<HTMLElement>();
const groupsId = useId();
const {
  width: groupsWidth,
  minimum: groupsMin,
  maximum: groupsMax,
  enabled: canResizeGroups,
  key: resizeGroupsKey,
  start: resizeGroupsStart,
} = useLocalGroupWidth(browser, () => query.value.group !== 'list');
const ownsVisit = props.model.captureVisit();
const directoryTrigger = ref<HTMLButtonElement>();
const directoryOrigin = shallowRef<HTMLElement>();
const directoriesOpen = ref(false);
const activeRoots = computed(() =>
  (view.value?.roots ?? []).filter((root) => root.checked !== false),
);
function openDirectories(event: MouseEvent) {
  if (!ownsVisit() || busy.value || !(event.currentTarget instanceof HTMLElement)) return;
  directoryOrigin.value = event.currentTarget;
  directoriesOpen.value = true;
}
function applyDirectories() {
  if (ownsVisit()) props.model.filter({ offset: 0, revision: null });
}

watch(canResizeGroups, async (enabled) => {
  const separator = groupSeparator.value;
  if (enabled || !separator || document.activeElement !== separator) return;
  await nextTick();
  const active = document.activeElement;
  if (!ownsVisit() || canResizeGroups.value || (active !== separator && active !== document.body))
    return;
  const selected = browser.value?.querySelector<HTMLButtonElement>(
    '.local-groups button[aria-pressed="true"]',
  );
  (selected ?? searchInput.value)?.focus({ preventScroll: true });
});
const expandedFolders = shallowRef(new Set(props.batchReturn?.checkpoint.expandedFolders));
let batchRestored = false;
function restoreBatchReturn() {
  const saved = props.batchReturn;
  const root = pageRoot.value;
  if (!saved || batchRestored || !root || loading.value || (!view.value && !readError.value))
    return;
  if (!ownsVisit()) return;
  batchRestored = true;
  if (saved.checkpoint.queryKey === localQueryKey(query.value)) {
    root.scrollTop = saved.checkpoint.scrollTop;
    if (groupList.value) groupList.value.scrollTop = saved.checkpoint.groupScrollTop;
  }
  const active = document.activeElement;
  if (active !== document.body && active !== saved.focusFrom) return;
  root
    .querySelector<HTMLElement>('[aria-label="更多本地音乐操作"]')
    ?.focus({ preventScroll: true });
}
onMounted(restoreBatchReturn);
watch([view, loading, readError], restoreBatchReturn, { flush: 'post' });
let groupFocus: { key: string | null; queryKey: string } | undefined;
function selectGroup(key: string | null) {
  const active = document.activeElement;
  const focused =
    active instanceof HTMLButtonElement &&
    browser.value?.contains(active) &&
    (key === null
      ? active.hasAttribute('data-local-all-groups')
      : active.dataset.localGroupKey === key);
  search.refine({ groupKey: key });
  groupFocus = focused ? { key, queryKey: localQueryKey(query.value) } : undefined;
}
watch(
  [view, readError, query],
  () => {
    const restore = groupFocus;
    if (!restore) return;
    if (restore.queryKey !== localQueryKey(query.value)) {
      groupFocus = undefined;
      return;
    }
    if (!view.value && !readError.value) return;
    groupFocus = undefined;
    if (!ownsVisit() || document.activeElement !== document.body) return;
    const group =
      restore.key === null
        ? browser.value?.querySelector<HTMLButtonElement>('[data-local-all-groups]')
        : Array.from(
            browser.value?.querySelectorAll<HTMLButtonElement>('[data-local-group-key]') ?? [],
          ).find((node) => node.dataset.localGroupKey === restore.key);
    const target =
      group ??
      pageRoot.value?.querySelector<HTMLButtonElement>('[data-local-read-error] button') ??
      searchInput.value;
    target?.focus({ preventScroll: true });
    target?.scrollIntoView({ block: 'nearest' });
  },
  { flush: 'post' },
);
const readLocked = computed(() => loading.value || !!readError.value || !view.value);
function startQueueDrop(event: PointerEvent, row: LocalTrack) {
  const hit = event.target instanceof Element ? event.target : undefined;
  if (hit?.closest('button, a, input, select') && !hit.closest('.local-title')) return;
  const revision = view.value?.revision;
  const scope = { ...query.value };
  const scopeKey = localQueryKey(scope);
  if (!revision) return;
  const current = () =>
    !!props.queueDropEnabled &&
    ownsVisit() &&
    !readLocked.value &&
    !busy.value &&
    keyword.value === query.value.keyword &&
    localQueryKey(query.value) === scopeKey &&
    view.value?.revision === revision &&
    view.value.tracks.some(
      (candidate) =>
        candidate.track.id === row.track.id &&
        candidate.revision === row.revision &&
        !candidate.track.missing,
    );
  if (current())
    emit('queueDrag', event, {
      kind: 'local',
      query: scope,
      revision,
      trackId: row.track.id,
      current,
    });
}
const canPlay = computed(
  () => props.playable && !busy.value && !readLocked.value && !!view.value?.playable,
);
const trackMenu = ref<InstanceType<typeof ActionMenu>>();
const trackTarget = shallowRef<{
  id: string;
  revision: string;
  queryKey: string;
  listRevision: string;
}>();
const openingDirectory = ref(false);
const directoryError = ref('');
const trackRow = computed(() => {
  const target = trackTarget.value;
  if (
    !target ||
    readLocked.value ||
    busy.value ||
    keyword.value !== query.value.keyword ||
    target.queryKey !== localQueryKey(query.value) ||
    target.listRevision !== view.value?.revision
  )
    return;
  return view.value?.tracks.find(
    (row) => row.track.id === target.id && row.revision === target.revision,
  );
});
const trackCurrent = computed(() => !!trackRow.value);
const trackMenuItems = computed(() => [
  {
    key: 'play',
    label: '播放',
    icon: 'play' as const,
    disabled: !canPlay.value || !trackRow.value || trackRow.value.track.missing,
  },
  {
    key: 'next',
    label: '下一首播放',
    icon: 'next' as const,
    disabled: !props.queueable || adding.value || !trackRow.value || trackRow.value.track.missing,
  },
  {
    key: 'directory',
    label: '打开文件所在目录',
    icon: 'list' as const,
    disabled: !trackRow.value || openingDirectory.value || trackRow.value.track.missing,
  },
  {
    key: 'hide_track',
    label: '从列表中删除',
    icon: 'trash' as const,
    disabled: !trackCurrent.value || openingDirectory.value,
  },
]);
watch(trackCurrent, (current) => {
  if (!current) trackMenu.value?.close(trackMenu.value.containsFocus());
});
watch(
  () => localQueryKey(query.value),
  () => {
    trackTarget.value = undefined;
    directoryError.value = '';
  },
);
function showTrackMenu(event: MouseEvent | KeyboardEvent, row: LocalTrack) {
  if (
    event instanceof KeyboardEvent &&
    !(event.key === 'ContextMenu' || (event.shiftKey && event.key === 'F10'))
  )
    return;
  event.preventDefault();
  event.stopPropagation();
  if (
    readLocked.value ||
    busy.value ||
    openingDirectory.value ||
    !view.value ||
    keyword.value !== query.value.keyword ||
    !ownsVisit()
  )
    return;
  const trigger = (event.currentTarget as HTMLElement)
    .closest('tr')
    ?.querySelector<HTMLButtonElement>('[data-local-track-menu]');
  if (!trigger) return;
  trackTarget.value = {
    id: row.track.id,
    revision: row.revision,
    queryKey: localQueryKey(query.value),
    listRevision: view.value.revision,
  };
  directoryError.value = '';
  trigger.focus({ preventScroll: true });
  const bounds = trigger.getBoundingClientRect();
  const pointer = event instanceof MouseEvent && event.type === 'contextmenu' && event.button === 2;
  void trackMenu.value?.showAt(
    pointer ? event.clientX : bounds.left,
    pointer ? event.clientY : bounds.bottom,
    trigger,
  );
}
function handleTrackKey(event: KeyboardEvent, row: LocalTrack) {
  if (event.key !== 'Delete' && event.key !== 'Backspace') {
    showTrackMenu(event, row);
    return;
  }
  if (
    event.defaultPrevented ||
    event.isComposing ||
    event.altKey ||
    event.ctrlKey ||
    event.metaKey ||
    event.shiftKey ||
    !(event.target instanceof HTMLElement) ||
    event.target.isContentEditable ||
    event.target.closest('input, textarea, select, [role="textbox"]')
  )
    return;
  event.preventDefault();
  event.stopPropagation();
  if (event.repeat || openingDirectory.value || !view.value || !ownsVisit()) return;
  trackTarget.value = {
    id: row.track.id,
    revision: row.revision,
    queryKey: localQueryKey(query.value),
    listRevision: view.value.revision,
  };
  void trackAction('hide_track');
}
async function trackAction(action: string) {
  const target = trackTarget.value;
  if (!target || !trackCurrent.value || openingDirectory.value || !ownsVisit()) return;
  if (action === 'play') {
    play(target.id);
    return;
  }
  if (action === 'next') {
    if (props.queueable && !adding.value && !trackRow.value?.track.missing)
      await queueScope({ ...query.value }, target.listRevision, target.id);
    return;
  }
  if (action === 'hide_track') {
    await props.model.command({ kind: 'hide_track', id: target.id, revision: target.revision });
    await nextTick();
    if (
      ownsVisit() &&
      target.queryKey === localQueryKey(query.value) &&
      document.activeElement === document.body
    ) {
      const sameRow = Array.from(
        pageRoot.value?.querySelectorAll<HTMLElement>('[data-local-id]') ?? [],
      ).find((row) => row.dataset.localId === target.id);
      const fallback = sameRow?.querySelector<HTMLButtonElement>('[data-local-track-menu]');
      (fallback ?? searchInput.value)?.focus({ preventScroll: true });
    }
    return;
  }
  if (action !== 'directory' || trackRow.value?.track.missing) return;
  openingDirectory.value = true;
  directoryError.value = '';
  try {
    await props.model.openTrackDirectory(target.id, target.revision);
  } catch {
    if (
      ownsVisit() &&
      trackTarget.value === target &&
      target.queryKey === localQueryKey(query.value)
    )
      directoryError.value = '无法打开文件所在目录，请检查文件或重新扫描后重试。';
  } finally {
    openingDirectory.value = false;
  }
}
const adding = ref(false);
const queueNotice = ref('');
const queueError = ref('');
const canAdd = computed(
  () =>
    props.queueable &&
    !busy.value &&
    !readLocked.value &&
    !adding.value &&
    keyword.value === query.value.keyword &&
    !!view.value?.playable &&
    view.value.playable <= 10000,
);
const queueMenu = computed(() => [
  {
    key: 'queue_all',
    label: '添加全部至播放列表',
    icon: 'plus' as const,
    disabled: !canAdd.value,
    hint: (view.value?.playable ?? 0) > 10000 ? '请缩小筛选范围至 10000 首以内' : undefined,
  },
  { key: 'batch', label: '批量操作', icon: 'list' as const, disabled: busy.value },
]);
const groupMenu = ref<InstanceType<typeof ActionMenu>>();
const groupScope = shallowRef<{ query: LocalQuery; revision: string; ownerKey: string }>();
const groupScopeCurrent = computed(
  () =>
    !!groupScope.value &&
    !readLocked.value &&
    !busy.value &&
    keyword.value === query.value.keyword &&
    groupScope.value.ownerKey === localQueryKey(query.value) &&
    groupScope.value.revision === view.value?.revision,
);
const groupMenuItems = computed(() => [
  {
    key: 'play',
    label: '播放',
    icon: 'play' as const,
    disabled: !groupScopeCurrent.value || !props.playable || adding.value,
  },
  {
    key: 'next',
    label: '下一首播放',
    icon: 'next' as const,
    disabled: !groupScopeCurrent.value || !props.queueable || adding.value,
  },
  ...(groupScope.value?.query.group === 'folder'
    ? [
        { key: 'folder-reveal-divider', label: '', separator: true as const },
        {
          key: 'reveal_folder',
          label: '打开文件所在目录',
          icon: 'folder' as const,
          disabled: !groupScopeCurrent.value || openingDirectory.value,
        },
      ]
    : []),
  ...(groupScope.value?.query.groupKey
    ? [
        ...(groupScope.value.query.group !== 'folder'
          ? [{ key: 'group-hide-divider', label: '', separator: true as const }]
          : []),
        {
          key: 'hide_group',
          label: '从列表中删除',
          icon: 'trash' as const,
          disabled: !groupScopeCurrent.value || adding.value || openingDirectory.value,
        },
      ]
    : []),
]);
function openGroupMenu(event: MouseEvent | KeyboardEvent) {
  if (
    event instanceof KeyboardEvent &&
    event.key !== 'ContextMenu' &&
    !(event.shiftKey && event.key === 'F10')
  )
    return;
  const target =
    event.target instanceof Element
      ? event.target.closest<HTMLButtonElement>('[data-local-group-key]')
      : null;
  const key = target?.dataset.localGroupKey;
  if (
    !target ||
    key === undefined ||
    !view.value ||
    readLocked.value ||
    keyword.value !== query.value.keyword
  )
    return;
  const scoped = localGroupQuery(query.value, view.value, key);
  if (!scoped) return;
  event.preventDefault();
  event.stopPropagation();
  groupScope.value = {
    query: scoped,
    revision: view.value.revision,
    ownerKey: localQueryKey(query.value),
  };
  const bounds = target.getBoundingClientRect();
  const point =
    event instanceof MouseEvent && (event.clientX || event.clientY)
      ? { x: event.clientX, y: event.clientY }
      : { x: bounds.left + 8, y: bounds.bottom };
  void groupMenu.value?.showAt(point.x, point.y, target);
}
async function groupAction(action: string) {
  const scope = groupScope.value;
  if (!ownsVisit() || !scope || !groupScopeCurrent.value) return;
  if (action === 'reveal_folder') {
    if (scope.query.group !== 'folder' || openingDirectory.value) return;
    openingDirectory.value = true;
    directoryError.value = '';
    try {
      await props.model.revealFolder(scope.query, scope.revision);
    } catch {
      if (ownsVisit() && groupScope.value === scope && groupScopeCurrent.value)
        directoryError.value = '无法定位文件夹，请检查目录或重新扫描后重试。';
    } finally {
      openingDirectory.value = false;
    }
    return;
  }
  if (adding.value) return;
  if (action === 'hide_group' && scope.query.groupKey && !openingDirectory.value) {
    const anchor = document.activeElement;
    await props.model.command({
      kind: 'hide_group',
      query: scope.query,
      revision: scope.revision,
    });
    await nextTick();
    // ActionMenu returns focus to its anchor before dispatch. If this very
    // button disappeared, restore focus without stealing it from another action.
    if (
      ownsVisit() &&
      groupScope.value === scope &&
      anchor instanceof HTMLButtonElement &&
      anchor.dataset.localGroupKey === scope.query.groupKey &&
      !anchor.isConnected &&
      document.activeElement === document.body
    ) {
      const selected = browser.value?.querySelector<HTMLButtonElement>(
        '.local-groups button[aria-pressed="true"]',
      );
      (selected ?? searchInput.value)?.focus({ preventScroll: true });
    }
    return;
  }
  if (action === 'play' && props.playable) emit('play', scope.query, scope.revision, null);
  else if (action === 'next' && props.queueable) await queueScope(scope.query, scope.revision);
}
function localMenuAction(key: string) {
  if (!ownsVisit()) return;
  if (key === 'queue_all' && canAdd.value && view.value) {
    void queueScope({ ...query.value }, view.value.revision);
  } else if (key === 'batch' && !busy.value) {
    const before = localQueryKey(query.value);
    search.flush();
    const queryKey = localQueryKey(query.value);
    emit(
      'batch',
      { ...query.value },
      {
        queryKey,
        scrollTop: queryKey === before ? (pageRoot.value?.scrollTop ?? 0) : 0,
        groupScrollTop: groupList.value?.scrollTop ?? 0,
        expandedFolders: [...expandedFolders.value],
      },
    );
  }
}
async function queueScope(captured: LocalQuery, revision: string, trackId?: string) {
  adding.value = true;
  queueNotice.value = queueError.value = '';
  try {
    if ((await props.queueLocal(captured, revision, trackId)) && ownsVisit())
      queueNotice.value = '已添加至播放列表';
  } catch (issue) {
    if (ownsVisit()) queueError.value = failure(issue).message;
  } finally {
    adding.value = false;
  }
}
defineExpose({ checkpoint: search.flush });
async function runAction(action: () => Promise<void>) {
  const origin = document.activeElement;
  await action();
  await nextTick();
  if (
    ownsVisit() &&
    origin instanceof HTMLElement &&
    pageRoot.value?.contains(origin) &&
    document.activeElement === document.body &&
    !origin.matches(':disabled')
  ) {
    origin.focus({ preventScroll: true });
    origin.scrollIntoView({ block: 'nearest' });
  }
}
async function clearSearch() {
  search.clear();
  await nextTick();
  if (ownsVisit()) searchInput.value?.focus({ preventScroll: true });
}
async function retryRead() {
  const origin = document.activeElement;
  await props.model.refresh();
  await nextTick();
  if (
    ownsVisit() &&
    origin instanceof HTMLElement &&
    pageRoot.value?.isConnected &&
    document.activeElement === document.body
  ) {
    const target = readError.value
      ? pageRoot.value.querySelector<HTMLElement>('[data-local-read-error] button:not(:disabled)')
      : searchInput.value;
    target?.focus({ preventScroll: true });
    target?.scrollIntoView({ block: 'nearest' });
  }
}
const tabs: { kind: LocalGroup; name: string }[] = [
  { kind: 'list', name: '歌曲' },
  { kind: 'artist', name: '歌手' },
  { kind: 'album', name: '专辑' },
  { kind: 'folder', name: '文件夹' },
];
const scanning = computed(() =>
  view.value?.roots.some((r) => r.enabled && ['pending', 'scanning'].includes(r.status)),
);
function play(id: string | null = null) {
  if (
    view.value &&
    canPlay.value &&
    (id === null || view.value.tracks.some((row) => row.track.id === id && !row.track.missing))
  )
    emit('play', { ...query.value }, view.value.revision, id);
}
const sortColumns = [
  { field: 'title', label: '音乐标题' },
  { field: 'artist', label: '歌手' },
  { field: 'album', label: '专辑' },
  { field: 'format', label: '格式' },
  { field: 'duration', label: '时长' },
] as const;
let sortFocus: { field: LocalSort; queryKey: string } | undefined;
function sort(field: LocalSort, event: MouseEvent) {
  const ownsFocus = document.activeElement === event.currentTarget;
  search.refine({
    sort: field,
    descending: query.value.sort === field ? !query.value.descending : false,
  });
  sortFocus = ownsFocus ? { field, queryKey: localQueryKey(query.value) } : undefined;
}
watch(
  [view, readError, query],
  () => {
    const restore = sortFocus;
    if (!restore) return;
    if (restore.queryKey !== localQueryKey(query.value)) {
      sortFocus = undefined;
      return;
    }
    if (!view.value && !readError.value) return;
    sortFocus = undefined;
    if (!ownsVisit() || document.activeElement !== document.body) return;
    const target =
      browser.value?.querySelector<HTMLButtonElement>(`[data-local-sort="${restore.field}"]`) ??
      pageRoot.value?.querySelector<HTMLButtonElement>('[data-local-read-error] button') ??
      searchInput.value;
    target?.focus({ preventScroll: true });
  },
  { flush: 'post' },
);
function status(root: LocalRoot) {
  if (!root.enabled) return '已暂停自动扫描';
  if (root.rootMissing) return '目录暂不可用，文件记录已保留';
  if (root.status === 'pending') return '等待扫描';
  if (root.status === 'scanning')
    return root.progress?.stage === 'probing'
      ? `正在处理 ${root.progress.processed} / ${root.progress.total}`
      : root.progress?.stage === 'committing'
        ? '正在保存列表'
        : '正在查找音乐';
  if (root.error) return root.error;
  return `已更新 ${root.updated}，未变化 ${root.unchanged}${root.failures ? `，${root.failures} 项无法读取` : ''}`;
}
</script>
<template>
  <section
    ref="pageRoot"
    class="local-page"
    aria-label="本地音乐"
    :data-local-loading="loading"
    :data-local-ready="!!view && !readError"
    :aria-busy="loading"
  >
    <header class="local-heading">
      <h1>本地音乐</h1>
      <span>共 {{ view?.total ?? 0 }} 首</span
      ><span role="status">{{ scanning ? '正在更新列表' : '' }}</span
      ><button ref="directoryTrigger" class="text-button" :disabled="busy" @click="openDirectories">
        选择目录
      </button>
    </header>
    <div class="local-toolbar">
      <button class="primary" :disabled="!canPlay" @click="play()">
        <Icon name="play" />播放全部
      </button>
      <button
        class="secondary"
        :disabled="
          !matching.ready.value ||
          matching.busy.value ||
          matching.unfinished.value ||
          readLocked ||
          !view?.playable
        "
        :title="matching.ready.value ? '匹配未关联的本地音乐' : '登录后可关联线上歌曲'"
        @click="matching.start"
      >
        匹配音乐
      </button>
      <button
        class="secondary"
        :disabled="busy || !view?.roots.some((r) => r.enabled)"
        @click="runAction(() => model.command({ kind: 'rescan' }))"
      >
        重新扫描
      </button>
      <ActionMenu
        variant="song"
        label="更多本地音乐操作"
        :items="queueMenu"
        @select="localMenuAction"
      />
      <form class="local-search" @submit.prevent="search.flush">
        <input
          ref="searchInput"
          :key="query.group"
          v-model="keyword"
          aria-label="搜索本地音乐"
          placeholder="搜索本地音乐"
          @input="search.input(($event.target as HTMLInputElement).value)"
          @compositionstart="search.compositionStart"
          @compositionend="search.compositionEnd(($event.target as HTMLInputElement).value)"
        />
        <button
          v-if="keyword"
          type="button"
          class="icon-button"
          aria-label="清空本地搜索"
          @click="clearSearch"
        >
          <Icon name="close" /></button
        ><button class="icon-button" aria-label="搜索本地音乐列表">
          <Icon name="search" />
        </button>
      </form>
    </div>
    <p v-if="adding || queueNotice" class="notice" role="status">
      {{ adding ? '正在添加本地音乐…' : queueNotice }}
    </p>
    <p v-if="directoryError" class="error-state" role="alert">{{ directoryError }}</p>
    <p v-if="queueError" class="error-state" role="alert">{{ queueError }}</p>
    <div v-if="matching.view.value?.job" class="local-match-progress" role="status">
      <span
        >{{
          {
            queued: '等待匹配',
            running: '正在匹配',
            paused: '匹配已暂停',
            failed: '匹配未完成，请重试',
            complete: '匹配完成',
            cancelled: '匹配已取消',
          }[matching.view.value.job.phase]
        }}
        · {{ matching.view.value.job.processed }} / {{ matching.view.value.job.total }}，成功
        {{ matching.view.value.job.matched
        }}<template v-if="matching.view.value.job.unmatched"
          >，未匹配 {{ matching.view.value.job.unmatched }}</template
        ><template v-if="matching.view.value.job.skipped"
          >，已跳过 {{ matching.view.value.job.skipped }}</template
        ></span
      >
      <progress
        v-if="matching.unfinished.value"
        :value="matching.view.value.job.processed"
        :max="matching.view.value.job.total || 1"
      />
      <button
        v-if="matching.running.value"
        class="text-button"
        :disabled="matching.busy.value"
        @click="matching.control('pause')"
      >
        暂停匹配
      </button>
      <button
        v-else-if="matching.unfinished.value"
        class="text-button"
        :disabled="matching.busy.value"
        @click="matching.control('resume')"
      >
        {{ matching.view.value.job.phase === 'failed' ? '重试匹配' : '继续匹配' }}
      </button>
      <button
        v-if="matching.unfinished.value"
        class="text-button"
        :disabled="matching.busy.value"
        @click="matching.control('cancel')"
      >
        取消匹配
      </button>
    </div>
    <p v-if="matching.error.value" class="error-banner" role="alert">
      {{ matching.error.value
      }}<button class="text-button" @click="matching.refresh">重新加载</button>
    </p>
    <div class="local-tabs" aria-label="本地音乐分类">
      <button
        v-for="tab in tabs"
        :key="tab.kind"
        :aria-pressed="query.group === tab.kind"
        @click="search.group(tab.kind)"
      >
        {{ tab.name }}</button
      ><label
        ><input
          type="checkbox"
          :checked="query.includeMissing"
          @change="search.refine({ includeMissing: ($event.target as HTMLInputElement).checked })"
        />显示不可用文件</label
      >
    </div>
    <details class="local-roots">
      <summary>管理音乐目录（{{ activeRoots.length }}）</summary>
      <p>自动扫描每 30 秒检查一次。暂停扫描保留列表和原文件。</p>
      <div v-for="root in activeRoots" :key="root.id" class="local-root">
        <div>
          <strong :title="root.path">{{ root.path }}</strong
          ><small>{{ status(root) }}</small
          ><progress
            v-if="root.status === 'scanning' && root.progress?.total"
            :value="root.progress.processed"
            :max="root.progress.total"
          />
        </div>
        <button
          class="text-button"
          :disabled="busy"
          @click="
            runAction(() =>
              model.command({ kind: root.enabled ? 'disable' : 'enable', id: root.id }),
            )
          "
        >
          {{ root.enabled ? '暂停扫描' : '恢复扫描' }}
        </button>
      </div>
    </details>
    <p v-if="notice" class="notice" role="status">{{ notice }}</p>
    <div v-if="error" class="error-banner" role="alert" data-local-command-error>
      {{ error }}
    </div>
    <div v-if="readError && view" class="error-banner" role="alert" data-local-read-error>
      {{ readError
      }}<button class="text-button" :disabled="loading" @click="retryRead">重试读取</button>
    </div>
    <div v-if="loading && !view" class="empty-state" role="status">
      <span class="spinner" />正在读取音乐库…
    </div>
    <div v-else-if="!view && readError" class="error-state" role="alert" data-local-read-error>
      <h2>本地音乐暂时无法读取</h2>
      <p>{{ readError }}</p>
      <button class="primary" :disabled="loading" @click="retryRead">重试读取</button>
    </div>
    <div v-else-if="!view?.total && !query.keyword && !query.groupKey" class="empty-state">
      <Icon name="music" />
      <h2>让收藏的音乐回到播放列表</h2>
      <p>选择音乐目录，支持 CUE 分轨和 NCM 文件。</p>
      <button class="primary" :disabled="busy" @click="openDirectories">选择音乐目录</button>
    </div>
    <div v-else ref="browser" class="local-browser">
      <aside
        v-if="query.group !== 'list'"
        ref="groupList"
        class="local-groups"
        :id="groupsId"
        :style="{ width: groupsWidth + 'px' }"
        aria-label="本地分组"
        @contextmenu="openGroupMenu"
        @keydown="openGroupMenu"
      >
        <button
          data-local-all-groups
          :aria-pressed="query.groupKey === null"
          @click="selectGroup(null)"
        >
          全部
        </button>
        <LocalFolderGroups
          v-if="query.group === 'folder'"
          v-model:expanded="expandedFolders"
          :groups="view?.groups ?? []"
          :selected="query.groupKey"
          :searching="!!query.keyword.trim()"
          @select="selectGroup"
        />
        <template v-else>
          <button
            v-for="group in namedGroups"
            :key="group.key"
            :aria-pressed="query.groupKey === group.key"
            :data-local-group-key="group.key"
            aria-haspopup="menu"
            aria-expanded="false"
            aria-keyshortcuts="Shift+F10"
            :title="group.key ? group.name : '其他'"
            @click="selectGroup(group.key)"
          >
            <span>{{ group.key ? group.name : '其他' }}</span
            ><small>{{ group.count }}</small>
          </button>
        </template>
        <ActionMenu
          ref="groupMenu"
          label="本地分组操作"
          variant="song"
          context-only
          :items="groupMenuItems"
          @select="groupAction"
        />
      </aside>
      <div
        v-if="query.group !== 'list'"
        ref="groupSeparator"
        class="local-group-split"
        :class="{ 'local-group-split-active': canResizeGroups }"
        :role="canResizeGroups ? 'separator' : undefined"
        :aria-hidden="canResizeGroups ? undefined : true"
        :tabindex="canResizeGroups ? 0 : undefined"
        :aria-label="canResizeGroups ? '调整本地分组宽度' : undefined"
        :aria-controls="canResizeGroups ? groupsId : undefined"
        :aria-orientation="canResizeGroups ? 'vertical' : undefined"
        :aria-valuemin="canResizeGroups ? groupsMin : undefined"
        :aria-valuemax="canResizeGroups ? groupsMax : undefined"
        :aria-valuenow="canResizeGroups ? groupsWidth : undefined"
        @pointerdown="resizeGroupsStart"
        @keydown="resizeGroupsKey"
      />
      <div class="local-tracks">
        <table>
          <thead>
            <tr>
              <th>#</th>
              <th
                v-for="{ field, label } in sortColumns"
                :key="field"
                scope="col"
                :aria-sort="
                  query.sort === field ? (query.descending ? 'descending' : 'ascending') : undefined
                "
              >
                <button type="button" :data-local-sort="field" @click="sort(field, $event)">
                  {{ label
                  }}<span v-if="query.sort === field" aria-hidden="true">
                    {{ query.descending ? '↓' : '↑' }}
                  </span>
                </button>
              </th>
              <th v-if="matching.ready.value">线上关联</th>
              <th class="local-track-actions" aria-label="操作" />
            </tr>
          </thead>
          <tbody>
            <tr
              v-for="(row, index) in view?.tracks"
              :key="row.track.id"
              :data-local-id="row.track.id"
              :class="{ current: currentId === row.track.id, unavailable: row.track.missing }"
              @dblclick="!row.track.missing && play(row.track.id)"
              @contextmenu="showTrackMenu($event, row)"
              @keydown="handleTrackKey($event, row)"
              @pointerdown="startQueueDrop($event, row)"
              @dragstart.prevent
            >
              <td>
                <button
                  class="icon-button"
                  :disabled="!canPlay || row.track.missing"
                  :aria-label="`播放本地歌曲 ${row.track.name}`"
                  @click="play(row.track.id)"
                >
                  <Icon v-if="currentId === row.track.id" name="volume" /><span v-else>{{
                    query.offset + index + 1
                  }}</span>
                </button>
              </td>
              <td>
                <button
                  class="local-title"
                  :disabled="!canPlay || row.track.missing"
                  :title="`${row.folder}/${row.fileName}`"
                  @click="play(row.track.id)"
                >
                  {{ row.track.name }}</button
                ><small v-if="row.cue" class="local-badge">CUE</small
                ><small v-if="row.encrypted" class="local-badge">NCM</small
                ><small v-if="row.track.missing">{{
                  row.unreadable ? '无法读取' : '文件缺失'
                }}</small>
              </td>
              <td :title="row.track.artists.join(' / ')">
                {{ row.track.artists.join(' / ') || '未知歌手' }}
              </td>
              <td :title="row.track.album">{{ row.track.album || '未知专辑' }}</td>
              <td>{{ row.format.toUpperCase() }}</td>
              <td>{{ time(row.track.durationMs / 1000) }}</td>
              <td v-if="matching.ready.value" class="local-online" @dblclick.stop>
                <button
                  class="text-button"
                  :disabled="
                    readLocked || row.track.missing || matching.busy.value || matching.running.value
                  "
                  :aria-label="`关联本地歌曲 ${row.track.name}`"
                  @click="matching.open(row)"
                >
                  {{ matching.link(row) ? '已关联' : '关联' }}</button
                ><button
                  v-if="matching.link(row)"
                  class="icon-button"
                  :disabled="readLocked"
                  :aria-label="`${row.track.name} 的线上评论`"
                  @click="
                    emit(
                      'comments',
                      matching.link(row)!.songId,
                      matching.link(row)?.track?.name || row.track.name,
                    )
                  "
                >
                  <Icon name="comment" />
                </button>
              </td>
              <td class="local-track-actions" @dblclick.stop>
                <button
                  class="icon-button"
                  type="button"
                  data-local-track-menu
                  aria-keyshortcuts="Delete Backspace"
                  aria-haspopup="menu"
                  :aria-label="`更多操作 ${row.track.name}`"
                  :disabled="readLocked || busy"
                  @click="showTrackMenu($event, row)"
                >
                  <Icon name="more" />
                </button>
              </td>
            </tr>
          </tbody>
        </table>
        <p v-if="!view?.tracks.length" class="empty-state">没有匹配的本地音乐</p>
      </div>
    </div>
    <ActionMenu
      ref="trackMenu"
      label="本地歌曲操作"
      variant="song"
      context-only
      :items="trackMenuItems"
      @select="trackAction"
    />
    <footer v-if="view?.total" class="local-pagination">
      <button
        class="text-button"
        :disabled="!query.offset || readLocked"
        @click="model.page(Math.max(0, query.offset - query.limit))"
      >
        上一页</button
      ><span
        >{{ Math.min(query.offset + 1, view.total) }}–{{
          Math.min(query.offset + query.limit, view.total)
        }}
        / {{ view.total }}</span
      ><button
        class="text-button"
        :disabled="!view.more || readLocked"
        @click="model.page(query.offset + query.limit)"
      >
        下一页
      </button>
    </footer>
  </section>
  <ModalTransition
    :return-focus="() => directoryOrigin ?? null"
    :fallback-focus="() => directoryTrigger ?? null"
  >
    <LocalDirectoriesDialog
      v-if="directoriesOpen"
      :current="ownsVisit"
      @close="directoriesOpen = false"
      @applied="applyDirectories"
    />
  </ModalTransition>
  <LocalMatchDialog v-if="matching.target.value" :model="matching" />
</template>
