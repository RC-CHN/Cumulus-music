<script setup lang="ts">
import { ref, computed, watch, onUnmounted } from 'vue';
import ModalTransition from './ModalTransition.vue';
import type { CloudDeleteTarget } from '../composables/useCloudDelete';
import Icon from './Icon.vue';
import CloudUploadDialog from './CloudUploadDialog.vue';
import CloudDeleteDialog from './CloudDeleteDialog.vue';
import type { CloudModel } from '../composables/useCloud';
import type { TrackView } from '../services/contracts';
import {
  cloudBytes,
  cloudIssue,
  deletionMessage,
  nextCloudSort,
  type CloudUpload,
  type CloudQuery,
} from '../services/cloud';
const props = defineProps<{ model: CloudModel; playable: boolean; currentId?: string }>();
const emit = defineEmits<{
  play: [query: CloudQuery, revision: string, startId: string | null];
  dialog: [open: boolean];
  batch: [];
  download: [tracks: TrackView[], revision: string];
}>();
const { page: view, query, busy, loading, error, readError, notice } = props.model;
const dialogOwner = ref(props.model.accountEpoch.value);
const pageRoot = ref<HTMLElement>();
function fallbackFocus() {
  return pageRoot.value?.querySelector<HTMLInputElement>('[aria-label="搜索云盘歌曲"]') ?? null;
}
const keyword = ref(query.value.keyword),
  selected = ref(new Set<string>()),
  uploadOpen = ref(false),
  deletion = ref<CloudDeleteTarget>();
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
    selected.value = new Set();
    if (pageRoot.value) pageRoot.value.scrollTop = 0;
  },
);
watch(
  ids,
  (rows) => (selected.value = new Set([...selected.value].filter((id) => rows.includes(id)))),
);
watch(
  () => uploadOpen.value || !!deletion.value,
  (value) => emit('dialog', value),
);
watch(
  () => props.model.accountEpoch.value,
  () => {
    uploadOpen.value = false;
    deletion.value = undefined;
    keyword.value = query.value.keyword;
  },
);
watch(
  () => query.value.keyword,
  (value) => (keyword.value = value),
);
onUnmounted(() => emit('dialog', false));
function openUpload() {
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
function play(id: string | null = null) {
  if (props.playable && view.value?.revision)
    emit('play', { ...query.value }, view.value.revision, id);
}
function remove() {
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
function download() {
  if (view.value?.revision && selected.value.size)
    emit(
      'download',
      tracks.value.filter((row) => selected.value.has(row.track.id)).map((row) => row.track),
      view.value.revision,
    );
}
function group(kind: 'pause' | 'resume') {
  if (!selected.value.size) {
    void props.model.command(
      kind === 'pause' ? { kind: 'pause_all' } : { kind: 'resume_all', retry_rejected: true },
    );
    return;
  }
  const rows = uploads.value.filter(
    (v) => selected.value.has(v.id) && (kind === 'pause' ? v.canPause : v.canResume),
  );
  if (rows.length)
    void props.model.command(
      kind === 'pause'
        ? { kind, ids: rows.map((v) => v.id) }
        : { kind, ids: rows.map((v) => v.id), retry_rejected: true },
    );
}
function control(task: CloudUpload, kind: 'pause' | 'resume' | 'cancel' | 'dismiss') {
  void props.model.command(
    kind === 'resume' ? { kind, ids: [task.id], retry_rejected: true } : { kind, ids: [task.id] },
  );
}
</script>
<template>
  <section
    ref="pageRoot"
    class="cloud-page"
    aria-label="音乐云盘"
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
        :aria-selected="query.tab === 'uploading'"
        :tabindex="query.tab === 'uploading' ? 0 : -1"
        data-cloud-tab="uploading"
        @keydown="tabKey"
        :class="{ selected: query.tab === 'uploading' }"
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
          :disabled="!playable || !view?.total || !view.revision"
          @click="play()"
        >
          <Icon name="play" />播放全部</button
        ><button
          class="secondary cloud-upload-button"
          aria-label="上传音乐"
          title="上传音乐"
          :disabled="busy || full"
          @click="openUpload"
        >
          <Icon name="up" /></button
        ><button
          class="secondary"
          :disabled="busy || !view?.revision || !selected.size"
          @click="remove"
        >
          删除所选</button
        ><button class="secondary" :disabled="!selected.size || !view?.revision" @click="download">
          <Icon name="down" />下载所选
        </button></template
      ><template v-else
        ><button class="primary" :disabled="busy || full" @click="openUpload">
          <Icon name="up" />上传音乐</button
        ><button
          class="secondary"
          :disabled="busy || full || !view?.uploadingCount"
          @click="group('resume')"
        >
          {{ selected.size ? '继续所选' : '全部继续' }}</button
        ><button
          class="secondary"
          :disabled="busy || !view?.uploadingCount"
          @click="group('pause')"
        >
          {{ selected.size ? '暂停所选' : '全部暂停' }}
        </button></template
      >
      <button
        v-if="query.tab === 'uploaded'"
        data-cloud-batch-open
        class="secondary"
        :disabled="busy || loading || !view?.revision"
        @click="emit('batch')"
      >
        批量操作
      </button>
      <form class="cloud-search" role="search" @submit.prevent="model.filter({ keyword })">
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
        class="icon-button"
        aria-label="刷新云盘"
        :disabled="loading"
        @click="model.refresh(true)"
      >
        <Icon name="refresh" />
      </button>
    </div>
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
                  :disabled="!playable"
                  @click="play(row.track.id)"
                >
                  <span>{{ view!.offset + index + 1 }}</span
                  ><Icon name="play" />
                </button>
              </td>
              <td class="cloud-track-title">
                <strong :title="row.track.name">{{ row.track.name }}</strong
                ><span
                  ><span class="cloud-artist" :title="row.track.artists.join(' / ')">{{
                    row.track.artists.join(' / ') || '未知歌手'
                  }}</span
                  ><small v-if="row.bitrate >= 900000">无损</small></span
                >
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
    <template v-else-if="view"
      ><div v-if="uploads.length" class="cloud-upload-list">
        <div class="cloud-upload-select">
          <input
            type="checkbox"
            aria-label="选择本页全部上传任务"
            :checked="selected.size === ids.length && !!ids.length"
            @change="all"
          /><span>歌曲</span><span>上传状态</span>
        </div>
        <article v-for="task in uploads" :key="task.id" class="cloud-upload-row">
          <input
            type="checkbox"
            :aria-label="`选择上传任务 ${task.title}`"
            :checked="selected.has(task.id)"
            @change="toggle(task.id)"
          />
          <div class="cloud-upload-name">
            <strong :title="task.title">{{ task.title || task.fileName }}</strong
            ><small :title="task.fileName">{{ task.fileName }}</small>
            <p v-if="task.error" class="cloud-error">{{ cloudIssue(task.error) }}</p>
            <small v-if="task.attachments.length">含 {{ task.attachments.length }} 个附件</small>
          </div>
          <div class="cloud-upload-progress">
            <span>{{ task.label }}</span
            ><progress
              :value="task.bytes"
              :max="task.total || 1"
              :aria-label="`${task.title} 上传进度`"
            /><small>{{ cloudBytes(task.bytes) }} / {{ cloudBytes(task.total) }}</small>
          </div>
          <div class="cloud-upload-actions">
            <button
              v-if="task.canPause"
              class="text-button"
              :disabled="busy"
              @click="control(task, 'pause')"
            >
              暂停</button
            ><button
              v-if="task.canResume"
              class="text-button"
              :disabled="busy"
              @click="control(task, 'resume')"
            >
              继续</button
            ><button
              v-if="task.canReconcile"
              class="text-button"
              :disabled="busy"
              @click="model.command({ kind: 'reconcile_upload', id: task.id })"
            >
              核对结果</button
            ><button
              v-if="task.canRenew"
              class="text-button"
              :disabled="busy"
              @click="model.command({ kind: 'renew', id: task.id })"
            >
              更新上传凭证</button
            ><button
              v-if="task.canCancel"
              class="text-button"
              :disabled="busy"
              @click="control(task, 'cancel')"
            >
              取消</button
            ><button
              v-if="task.canDismiss"
              class="text-button"
              :disabled="busy"
              @click="control(task, 'dismiss')"
            >
              清除记录
            </button>
          </div>
        </article>
      </div>
      <div v-else class="empty-state">
        <Icon name="up" />
        <h2>还没有上传任务</h2>
        <p>选择音频文件，开始建立你的音乐云盘</p>
        <button class="primary" :disabled="busy || full" @click="openUpload">上传音乐</button>
      </div></template
    >
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
    >
      <CloudDeleteDialog
        v-if="deletion"
        :model="model"
        :target="deletion"
        @close="deletion = undefined"
      />
    </ModalTransition>
  </section>
</template>
