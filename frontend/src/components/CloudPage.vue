<script setup lang="ts">
import { ref, computed, watch, onUnmounted, nextTick } from 'vue';
import ModalTransition from './ModalTransition.vue';
import type { CloudDeleteTarget } from '../composables/useCloudDelete';
import Icon from './Icon.vue';
import CloudTaskTable from './cloud/CloudTaskTable.vue';
import CloudTasksToolbar from './cloud/CloudTasksToolbar.vue';
import CloudTaskRemoveDialog from './cloud/CloudTaskRemoveDialog.vue';
import { cloudTaskTarget, type CloudTaskTarget } from '../services/cloudTasks';
import CloudUploadDialog from './CloudUploadDialog.vue';
import CloudDeleteDialog from './CloudDeleteDialog.vue';
import CloudRowMenu from './cloud/CloudRowMenu.vue';
import CloudToolbarMenu from './cloud/CloudToolbarMenu.vue';
import PlaylistCollectDialog from './PlaylistCollectDialog.vue';
import type { PlayerModel } from '../composables/usePlayer';
import type { PlaylistManager } from '../composables/usePlaylists';
import type { CloudMenuTarget } from '../services/cloudMenu';
import type { CloudModel } from '../composables/useCloud';
import { failure, type PlaylistView, type TrackView } from '../services/contracts';
import {
  cloudBytes,
  deletionMessage,
  nextCloudSort,
  type CloudUpload,
  type CloudQuery,
  type CloudCommand,
} from '../services/cloud';
const props = defineProps<{
  model: CloudModel;
  playable: boolean;
  currentId?: string;
  player: PlayerModel;
  manager: PlaylistManager;
  playlists: PlaylistView[];
}>();
const emit = defineEmits<{
  dialog: [open: boolean];
  batch: [];
  download: [tracks: TrackView[], revision: string];
  expired: [];
}>();
const { page: view, query, busy, loading, error, readError, notice } = props.model;
const dialogOwner = ref(props.model.accountEpoch.value);
const pageRoot = ref<HTMLElement>();
const toolbar = ref<InstanceType<typeof CloudToolbarMenu>>(),
  rowMenu = ref<InstanceType<typeof CloudRowMenu>>(),
  rowDialogOpen = ref(false),
  collecting = ref<{ epoch: string; tracks: TrackView[] }>(),
  dialogReturn = ref<HTMLElement>();
function fallbackFocus() {
  const root = pageRoot.value;
  return (
    root?.querySelector<HTMLElement>('[aria-label="搜索云盘歌曲"]') ??
    root?.querySelector<HTMLElement>('.cloud-task-toolbar button:not(:disabled)') ??
    root?.querySelector<HTMLElement>('.cloud-task-history-entry:not(:disabled)') ??
    root?.querySelector<HTMLElement>('[role="tab"][aria-selected="true"]') ??
    null
  );
}
const keyword = ref(query.value.keyword),
  selected = ref(new Set<string>()),
  uploadOpen = ref(false),
  deletion = ref<CloudDeleteTarget>(),
  taskRemoval = ref<CloudTaskTarget>();
const tracks = computed(() => view.value?.tracks ?? []),
  uploads = computed(() => view.value?.uploads ?? []);
const ids = computed(() =>
  query.value.tab === 'uploaded'
    ? tracks.value.map((t) => t.track.id)
    : uploads.value.map((t) => t.id),
);
const metadata = computed(() => props.model.view.value);
const full = computed(
  () =>
    !!metadata.value?.maxSize &&
    metadata.value.size !== null &&
    metadata.value.size >= metadata.value.maxSize,
);
const capacity = computed(() =>
  metadata.value?.maxSize && metadata.value.size !== null
    ? Math.min(100, (metadata.value.size / metadata.value.maxSize) * 100)
    : 0,
);
watch(
  () => [
    query.value.tab,
    query.value.offset,
    query.value.keyword,
    query.value.sort,
    query.value.descending,
    props.model.accountEpoch.value,
  ],
  () => {
    rowMenu.value?.close();
    selected.value = new Set();
    if (pageRoot.value) pageRoot.value.scrollTop = 0;
  },
);
// A background native completion can remove the focused row between polls.
// Only restore when that exact element vanished and the user has not moved on.
watch(uploads, async () => {
  const origin = document.activeElement,
    owner = props.model.accountEpoch.value,
    host = pageRoot.value;
  if (
    !(origin instanceof HTMLElement) ||
    !host?.contains(origin) ||
    !origin.closest('.cloud-task-table,.cloud-task-toolbar')
  )
    return;
  await nextTick();
  if (
    owner === props.model.accountEpoch.value &&
    host.isConnected &&
    document.hasFocus() &&
    (!origin.isConnected || origin.matches(':disabled')) &&
    (document.activeElement === document.body || document.activeElement === origin)
  )
    fallbackFocus()?.focus({ preventScroll: true });
});
watch(
  ids,
  (rows) => (selected.value = new Set([...selected.value].filter((id) => rows.includes(id)))),
);
watch(
  () =>
    uploadOpen.value ||
    !!deletion.value ||
    !!collecting.value ||
    rowDialogOpen.value ||
    !!taskRemoval.value,
  (value) => emit('dialog', value),
);
watch(
  () => props.model.accountEpoch.value,
  () => {
    uploadOpen.value = false;
    deletion.value = undefined;
    taskRemoval.value = undefined;
    collecting.value = undefined;
    dialogReturn.value = undefined;
    rowMenu.value?.close();
    keyword.value = query.value.keyword;
  },
);
watch(
  () => query.value.keyword,
  (value) => (keyword.value = value),
);
onUnmounted(() => emit('dialog', false));
function openUpload() {
  if (toolbar.value?.busy || busy.value) return;
  dialogReturn.value = undefined;
  dialogOwner.value = props.model.accountEpoch.value;
  uploadOpen.value = true;
}
function clearSearch() {
  keyword.value = '';
  props.model.filter({ keyword: '' });
}
function tabKey(event: KeyboardEvent) {
  const keys: Record<string, string> = {
    ArrowLeft: 'uploaded',
    ArrowRight: 'uploading',
    Home: 'uploaded',
    End: 'uploading',
  };
  const tab = keys[event.key];
  if (!tab) return;
  event.preventDefault();
  const next = (event.currentTarget as HTMLElement).parentElement?.querySelector<HTMLButtonElement>(
    `[data-cloud-tab="${tab}"]`,
  );
  next?.focus();
  next?.click();
}
function toggle(id: string) {
  const next = new Set(selected.value);
  next.has(id) ? next.delete(id) : next.add(id);
  selected.value = next;
}
function all() {
  selected.value = selected.value.size === ids.value.length ? new Set() : new Set(ids.value);
}
function sort(value: Exclude<CloudQuery['sort'], 'natural'>) {
  props.model.filter(nextCloudSort(query.value, value));
}
function sortState(column: CloudQuery['sort']): 'none' | 'ascending' | 'descending' {
  return query.value.sort === column || (column === 'title' && query.value.sort === 'artist')
    ? query.value.descending
      ? 'descending'
      : 'ascending'
    : 'none';
}
function indicator(column: CloudQuery['sort']) {
  if (sortState(column) === 'none') return '';
  return `${column === 'title' && query.value.sort === 'artist' ? '歌手 ' : ''}${query.value.descending ? '↓' : '↑'}`;
}
function date(ms: number) {
  return ms ? new Date(ms).toLocaleDateString('zh-CN') : '—';
}
async function play(id: string | null = null) {
  if (
    !props.playable ||
    props.player.busy.value ||
    busy.value ||
    loading.value ||
    !view.value?.revision
  )
    return;
  const owner = props.model.accountEpoch.value,
    revision = view.value.revision,
    captured = { ...query.value };
  props.model.error.value = '';
  props.model.notice.value = '正在准备播放…';
  try {
    const value = await props.player.selectForPage({
      kind: 'cloud',
      query: captured,
      revision,
      start_id: id,
      quality: props.player.view.value?.qualityPreferences.quality ?? 'standard',
    });
    if (
      owner === props.model.accountEpoch.value &&
      JSON.stringify(captured) === JSON.stringify(query.value)
    )
      props.model.notice.value = value ? '正在播放云盘歌曲' : '本次播放操作已更新或取消';
  } catch (e) {
    if (
      owner !== props.model.accountEpoch.value ||
      revision !== view.value?.revision ||
      JSON.stringify(captured) !== JSON.stringify(query.value)
    )
      return;
    const issue = failure(e);
    props.model.notice.value = '';
    props.model.error.value = issue.message;
    if (['session_expired', 'profile_changed'].includes(issue.code)) emit('expired');
  }
}
function remove() {
  dialogReturn.value = undefined;
  if (view.value?.revision && selected.value.size) {
    dialogOwner.value = props.model.accountEpoch.value;
    deletion.value = {
      epoch: dialogOwner.value,
      ids: [...selected.value],
      titles: tracks.value
        .filter((row) => selected.value.has(row.track.id))
        .map((row) => row.track.name),
      revision: view.value.revision,
    };
  }
}
function collectRows(target: CloudMenuTarget, origin: HTMLElement | undefined) {
  dialogOwner.value = target.epoch;
  dialogReturn.value = origin;
  collecting.value = { epoch: target.epoch, tracks: target.tracks.map((row) => row.track) };
}
function deleteRows(target: CloudMenuTarget, origin: HTMLElement | undefined) {
  dialogOwner.value = target.epoch;
  dialogReturn.value = origin;
  deletion.value = {
    epoch: target.epoch,
    revision: target.revision,
    ids: target.tracks.map((row) => row.track.id),
    titles: target.tracks.map((row) => row.track.name),
  };
}
function download() {
  if (view.value?.revision && selected.value.size)
    emit(
      'download',
      tracks.value.filter((row) => selected.value.has(row.track.id)).map((row) => row.track),
      view.value.revision,
    );
}
function removeTasks(target: CloudTaskTarget, origin: HTMLElement | undefined) {
  if (busy.value) return;
  dialogOwner.value = target.epoch;
  dialogReturn.value = origin;
  taskRemoval.value = target;
}
function removeTask(task: CloudUpload, origin: HTMLElement | undefined) {
  removeTasks(
    cloudTaskTarget(props.model.accountEpoch.value, [task], false, query.value.tab === 'history'),
    origin,
  );
}
async function taskCommand(command: CloudCommand, event?: MouseEvent) {
  const owner = props.model.accountEpoch.value,
    origin = event?.currentTarget as HTMLElement | undefined,
    host = pageRoot.value;
  if (origin && document.activeElement === origin) host?.focus({ preventScroll: true });
  await props.model.command(command);
  await nextTick();
  if (owner === props.model.accountEpoch.value && document.activeElement === host)
    (origin?.isConnected && !origin.matches(':disabled') ? origin : fallbackFocus())?.focus({
      preventScroll: true,
    });
}
</script>
<template>
  <section
    ref="pageRoot"
    class="cloud-page"
    tabindex="-1"
    aria-label="音乐云盘"
    :data-cloud-epoch="model.accountEpoch.value"
    :data-cloud-loading="loading"
    :data-cloud-ready="!!view"
  >
    <div class="cloud-heading">
      <h1>音乐云盘</h1>
    </div>
    <div class="download-tabs" role="tablist" aria-label="云盘分类">
      <button
        role="tab"
        :aria-selected="query.tab === 'uploaded'"
        :tabindex="query.tab === 'uploaded' ? 0 : -1"
        data-cloud-tab="uploaded"
        @keydown="tabKey"
        :class="{ selected: query.tab === 'uploaded' }"
        @click="model.filter({ tab: 'uploaded' })"
      >
        已上传单曲 <span>{{ view?.revision ? view.uploadedCount : '—' }}</span></button
      ><button
        role="tab"
        :aria-selected="query.tab !== 'uploaded'"
        :tabindex="query.tab !== 'uploaded' ? 0 : -1"
        data-cloud-tab="uploading"
        @keydown="tabKey"
        :class="{ selected: query.tab !== 'uploaded' }"
        @click="model.filter({ tab: 'uploading' })"
      >
        正在上传 <span>{{ view?.uploadingCount ?? '—' }}</span>
      </button>
    </div>
    <div class="cloud-capacity" aria-label="网盘容量">
      <span>网盘容量</span><progress :value="capacity" max="100" aria-label="云盘空间使用量" />
      <span
        >{{ cloudBytes(metadata?.size ?? null) }} /
        {{ cloudBytes(metadata?.maxSize ?? null) }}</span
      >
      <span v-if="metadata?.revision && !full" class="cloud-capacity-tip"
        >歌曲永久保存，随时随地多端畅听</span
      >
    </div>
    <div class="cloud-toolbar">
      <template v-if="query.tab === 'uploaded'"
        ><button
          class="primary"
          :disabled="
            !playable || player.busy.value || busy || loading || !view?.total || !view.revision
          "
          @click="play()"
        >
          <Icon name="play" />播放全部</button
        ><button
          class="secondary cloud-upload-button"
          aria-label="上传音乐"
          title="上传音乐"
          :disabled="busy || full || toolbar?.busy"
          @click="openUpload"
        >
          <Icon name="upload" /></button
        ><CloudToolbarMenu
          ref="toolbar"
          :model="model"
          :player="player"
          :disabled="busy || loading || uploadOpen || !!deletion || !!collecting || rowDialogOpen"
          @batch="emit('batch')"
          @download="(tracks, revision) => emit('download', tracks, revision)"
          @expired="emit('expired')"
        /><button
          v-if="selected.size"
          class="secondary"
          :disabled="busy || !view?.revision || toolbar?.busy"
          @click="remove"
        >
          删除所选</button
        ><button
          v-if="selected.size"
          class="secondary"
          :disabled="busy || !view?.revision || toolbar?.busy"
          @click="download"
        >
          <Icon name="download" />下载所选
        </button></template
      ><CloudTasksToolbar
        v-else
        :key="model.accountEpoch.value"
        :model="model"
        :disabled="busy || loading || !!taskRemoval"
        :full="full"
        :history="query.tab === 'history'"
        @remove="removeTasks"
        @expired="emit('expired')"
      />
      <form
        v-if="query.tab === 'uploaded'"
        class="cloud-search"
        role="search"
        @submit.prevent="model.filter({ keyword })"
      >
        <input
          v-model="keyword"
          aria-label="搜索云盘歌曲"
          placeholder="搜索云盘歌曲"
          maxlength="256"
        />
        <button
          v-if="keyword"
          type="button"
          class="icon-button"
          aria-label="清空云盘搜索"
          @click="clearSearch"
        >
          <Icon name="close" />
        </button>
        <button type="submit" class="icon-button" aria-label="搜索云盘列表">
          <Icon name="search" />
        </button>
      </form>
      <button
        v-if="query.tab === 'uploading'"
        class="text-button cloud-task-history-entry"
        :disabled="busy"
        @click="model.filter({ tab: 'history' })"
      >
        上传记录 <span>{{ metadata?.historyCount ?? 0 }}</span>
      </button>
      <button
        class="icon-button"
        aria-label="刷新云盘"
        :disabled="loading"
        @click="model.refresh(true)"
      >
        <Icon name="refresh" />
      </button>
    </div>
    <h2 v-if="query.tab === 'history'" class="cloud-task-history-heading">上传记录</h2>
    <p v-if="toolbar?.message" class="cloud-notice cloud-toolbar-feedback" role="status">
      {{ toolbar.message }}
    </p>
    <p v-if="toolbar?.error" class="cloud-error cloud-toolbar-feedback failed" role="alert">
      {{ toolbar.error }}
      <button
        class="text-button"
        :disabled="toolbar.busy"
        @click="toolbar.needsRefresh ? toolbar.refresh() : toolbar.retry()"
      >
        {{ toolbar.needsRefresh ? '刷新云盘列表' : '重新执行' }}
      </button>
      <button class="text-button" :disabled="toolbar.busy" @click="toolbar.clear()">关闭</button>
    </p>
    <p v-if="full" class="cloud-error" role="status">云盘空间已满，请清理部分歌曲后再上传。</p>
    <p v-if="notice" class="cloud-notice" role="status">{{ notice }}</p>
    <p v-if="error || (readError && view) || view?.workerError" class="cloud-error" role="alert">
      {{ error || readError || view?.workerError }}
      <button v-if="readError" class="text-button" @click="model.refresh(true)">重新加载</button>
    </p>
    <div
      v-if="loading && (!view || (query.tab === 'uploaded' && !view.revision))"
      class="empty-state"
      role="status"
    >
      <span class="spinner" />
      <p>正在读取音乐云盘…</p>
    </div>
    <div
      v-else-if="readError && (!view || (query.tab === 'uploaded' && !view.revision))"
      class="empty-state"
      role="alert"
    >
      <Icon name="cloud" />
      <h2>暂时无法读取音乐云盘</h2>
      <p>{{ readError }}</p>
      <button class="primary" :disabled="loading" @click="model.refresh(true)">重新加载</button>
    </div>
    <template v-else-if="query.tab === 'uploaded' && view">
      <div v-if="tracks.length" class="cloud-table-wrap">
        <table class="cloud-table">
          <thead>
            <tr>
              <th class="cloud-check">
                <input
                  type="checkbox"
                  aria-label="选择本页全部歌曲"
                  :checked="selected.size === ids.length && !!ids.length"
                  @change="all"
                />
              </th>
              <th class="cloud-index">#</th>
              <th :aria-sort="sortState('title')">
                <button data-cloud-sort="title" @click="sort('title')">
                  标题 <span>{{ indicator('title') }}</span>
                </button>
              </th>
              <th :aria-sort="sortState('album')">
                <button data-cloud-sort="album" @click="sort('album')">
                  专辑 <span>{{ indicator('album') }}</span>
                </button>
              </th>
              <th :aria-sort="sortState('format')">
                <button data-cloud-sort="format" @click="sort('format')">
                  格式 <span>{{ indicator('format') }}</span>
                </button>
              </th>
              <th :aria-sort="sortState('uploaded')">
                <button data-cloud-sort="uploaded" @click="sort('uploaded')">
                  上传时间 <span>{{ indicator('uploaded') }}</span>
                </button>
              </th>
              <th :aria-sort="sortState('size')">
                <button data-cloud-sort="size" @click="sort('size')">
                  大小 <span>{{ indicator('size') }}</span>
                </button>
              </th>
            </tr>
          </thead>
          <tbody>
            <tr
              v-for="(row, index) in tracks"
              :key="row.track.id"
              :data-cloud-id="row.track.id"
              :class="{ selected: selected.has(row.track.id), playing: currentId === row.track.id }"
              @contextmenu="rowMenu?.show($event, row)"
              @keydown="rowMenu?.show($event, row)"
              @dblclick="
                !($event.target as HTMLElement).closest('button,input') && play(row.track.id)
              "
            >
              <td>
                <input
                  type="checkbox"
                  :aria-label="`选择 ${row.track.name}`"
                  :checked="selected.has(row.track.id)"
                  @change="toggle(row.track.id)"
                />
              </td>
              <td>
                <button
                  class="cloud-row-play"
                  :aria-label="`播放 ${row.track.name}`"
                  :disabled="!playable || player.busy.value || busy || loading"
                  @click="play(row.track.id)"
                >
                  <span>{{ view!.offset + index + 1 }}</span
                  ><Icon name="play" />
                </button>
              </td>
              <td class="cloud-track-title">
                <div class="cloud-title-content">
                  <div class="cloud-title-copy">
                    <strong :title="row.track.name">{{ row.track.name }}</strong
                    ><span
                      ><span class="cloud-artist" :title="row.track.artists.join(' / ')">{{
                        row.track.artists.join(' / ') || '未知歌手'
                      }}</span
                      ><small v-if="row.bitrate >= 900000">无损</small></span
                    >
                  </div>
                  <button
                    class="icon-button cloud-more"
                    :aria-label="`更多操作：${row.track.name}`"
                    aria-haspopup="menu"
                    :disabled="
                      busy || loading || uploadOpen || !!deletion || !!collecting || rowDialogOpen
                    "
                    @click="rowMenu?.show($event, row)"
                  >
                    <Icon name="more" />
                  </button>
                </div>
              </td>
              <td class="cloud-album" :title="row.track.album">
                {{ row.track.album || '未知专辑' }}
              </td>
              <td class="cloud-format">{{ row.format.toUpperCase() || '—' }}</td>
              <td class="cloud-date">{{ date(row.addedAt) }}</td>
              <td class="cloud-size">{{ cloudBytes(row.fileSize) }}</td>
            </tr>
          </tbody>
        </table>
      </div>
      <div v-else class="empty-state">
        <Icon name="cloud" />
        <h2>{{ query.keyword ? '没有找到相关歌曲' : '把喜欢的音乐带在身边' }}</h2>
        <p>
          {{
            query.keyword
              ? '试试歌曲名、歌手、专辑或文件名'
              : '上传本地音乐，在音乐云盘中保存和收听'
          }}
        </p>
        <button v-if="!query.keyword" class="primary" :disabled="busy || full" @click="openUpload">
          上传音乐
        </button>
      </div>
    </template>
    <template v-else-if="view">
      <CloudTaskTable
        v-if="uploads.length"
        :key="`${model.accountEpoch.value}:${query.tab}`"
        :rows="uploads"
        :offset="view.offset"
        :epoch="model.accountEpoch.value"
        :disabled="busy || !!taskRemoval"
        @remove="removeTask"
        @command="taskCommand"
        @expired="emit('expired')"
      />
      <div v-else class="empty-state">
        <Icon :name="query.tab === 'history' ? 'history' : 'upload'" />
        <h2>{{ query.tab === 'history' ? '还没有上传记录' : '还没有上传任务' }}</h2>
        <p>
          {{
            query.tab === 'history'
              ? '完成和取消的任务会保留在这里'
              : '选择音频文件，开始建立你的音乐云盘'
          }}
        </p>
        <button
          v-if="query.tab !== 'history'"
          class="primary"
          :disabled="busy || full"
          @click="openUpload"
        >
          上传音乐
        </button>
      </div>
    </template>
    <div v-if="view && view.total > query.limit" class="cloud-pagination">
      <button
        class="secondary"
        :disabled="!view.offset || loading"
        @click="model.filter({ offset: Math.max(0, view.offset - query.limit) })"
      >
        上一页</button
      ><span
        >{{ Math.floor(view.offset / query.limit) + 1 }} /
        {{ Math.ceil(view.total / query.limit) }} · {{ view.total }} 首</span
      ><button
        class="secondary"
        :disabled="view.offset + query.limit >= view.total || loading"
        @click="model.filter({ offset: view.offset + query.limit })"
      >
        下一页
      </button>
    </div>
    <details v-if="view?.deletions.length" class="cloud-delete-history">
      <summary>最近的删除操作</summary>
      <div v-for="op in view.deletions" :key="op.id">
        <span>{{ deletionMessage(op) }}</span
        ><button
          v-if="['sending', 'verifying', 'uncertain'].includes(op.phase)"
          class="text-button"
          :disabled="busy"
          @click="model.command({ kind: 'reconcile_delete', id: op.id })"
        >
          只读核对
        </button>
      </div>
    </details>
    <ModalTransition
      :restore-focus="dialogOwner === model.accountEpoch.value"
      :fallback-focus="fallbackFocus"
      :return-focus="() => dialogReturn ?? null"
      ><CloudTaskRemoveDialog
        v-if="taskRemoval"
        :model="model"
        :target="taskRemoval"
        @close="taskRemoval = undefined"
    /></ModalTransition>
    <ModalTransition
      :restore-focus="dialogOwner === model.accountEpoch.value"
      :fallback-focus="fallbackFocus"
      :return-focus="() => dialogReturn ?? null"
    >
      <CloudUploadDialog
        v-if="uploadOpen"
        :model="model"
        @close="uploadOpen = false"
        @started="model.filter({ tab: 'uploading' })"
      />
    </ModalTransition>
    <ModalTransition
      :restore-focus="dialogOwner === model.accountEpoch.value"
      :fallback-focus="fallbackFocus"
      :return-focus="() => dialogReturn ?? null"
    >
      <CloudDeleteDialog
        v-if="deletion"
        :model="model"
        :target="deletion"
        @close="deletion = undefined"
      />
    </ModalTransition>
    <CloudRowMenu
      ref="rowMenu"
      :model="model"
      :player="player"
      :rows="tracks"
      :selected="selected"
      :disabled="
        busy ||
        loading ||
        !!toolbar?.busy ||
        uploadOpen ||
        !!deletion ||
        !!collecting ||
        rowDialogOpen
      "
      @collect="collectRows"
      @remove="deleteRows"
      @dialog="rowDialogOpen = $event"
      @expired="emit('expired')"
    />
    <ModalTransition
      :restore-focus="collecting?.epoch === model.accountEpoch.value"
      :return-focus="() => dialogReturn ?? null"
      :fallback-focus="fallbackFocus"
    >
      <PlaylistCollectDialog
        v-if="collecting"
        :manager="manager"
        :epoch="collecting.epoch"
        :tracks="collecting.tracks"
        :playlists="playlists"
        @close="collecting = undefined"
        @expired="emit('expired')"
      />
    </ModalTransition>
  </section>
</template>
