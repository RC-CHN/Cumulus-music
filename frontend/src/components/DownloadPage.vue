<script setup lang="ts">
import { computed, nextTick, ref, watch } from 'vue';
import type { DownloadModel } from '../composables/useDownloads';
import type { DownloadDialogTarget, DownloadTask, DownloadQuery } from '../services/downloads';
import type { PlaySelection } from '../services/player';
import {
  downloadSection,
  downloadTableColumns,
  downloadSectionQuery,
  nextDownloadSort,
  type DownloadSection,
} from '../services/downloadPage';
import DownloadTabs from './download/DownloadTabs.vue';
import DownloadToolbar from './download/DownloadToolbar.vue';
import DownloadTable from './download/DownloadTable.vue';
import DownloadMvGrid from './download/DownloadMvGrid.vue';
import DownloadOperations from './DownloadOperations.vue';
import Icon from './Icon.vue';
import '../styles/download-page.css';
const props = defineProps<{
  model: DownloadModel;
  playable: boolean;
  preparing?: boolean;
  currentId?: string;
}>();
const emit = defineEmits<{
  dialog: [target: DownloadDialogTarget];
  batch: [category: 'song' | 'voice'];
  select: [selection: PlaySelection];
  playMv: [task: DownloadTask];
}>();
const { view, query, busy, loading, error, readError, notice } = props.model;
const root = ref<HTMLElement>();
async function loadMore() {
  const origin = document.activeElement;
  await props.model.loadMore();
  await nextTick();
  if (
    origin instanceof HTMLElement &&
    !origin.isConnected &&
    document.activeElement === document.body
  ) {
    const scroll = root.value?.querySelector<HTMLElement>('.download-scroll,.download-mv-scroll');
    const bounds = scroll?.getBoundingClientRect();
    const rows = [
      ...(scroll?.querySelectorAll<HTMLElement>('[data-download-index],.download-mv-cover') ?? []),
    ];
    const visible =
      bounds &&
      rows.find((row) => {
        const r = row.getBoundingClientRect();
        return r.top >= bounds.top && r.bottom <= bounds.bottom;
      });
    (visible || scroll)?.focus({ preventScroll: true });
  }
}
async function filter(value: Partial<DownloadQuery>, selector: string) {
  const origin = document.activeElement;
  await props.model.filter(value);
  await nextTick();
  if (document.activeElement === document.body || document.activeElement === origin) {
    const target = root.value?.querySelector<HTMLElement>(
      readError.value ? '.error-state button' : selector,
    );
    (target ?? root.value?.querySelector<HTMLElement>('[role="tab"][aria-selected="true"]'))?.focus(
      { preventScroll: true },
    );
  }
}
const tasks = computed(() => view.value?.tasks ?? []);
function openBatch() {
  if (section.value === 'song' || section.value === 'voice') emit('batch', section.value);
}
const section = computed(() => downloadSection(query.value));
const columnGrowth = ref<number[]>([]);
watch(
  section,
  (value) => {
    columnGrowth.value = downloadTableColumns(value).map((c) => c.grow);
  },
  { immediate: true },
);
const locked = computed(
  () => busy.value || loading.value || !!readError.value || !!view.value?.migration,
);
const repairing = computed(
  () => new Set(view.value?.jobs.filter((j) => j.running).map((j) => j.id)),
);
const canPlay = computed(
  () =>
    !locked.value &&
    !props.preparing &&
    props.playable &&
    tasks.value.some((t) => t.source !== 'mv'),
);
function changeSection(value: DownloadSection) {
  if (value === section.value) return;
  props.model.filter(downloadSectionQuery(value));
}
function play(task?: DownloadTask) {
  if (task?.source === 'mv') {
    emit('playMv', task);
    return;
  }
  emit('select', { kind: 'downloads', query: { ...query.value }, start_id: task?.id ?? null });
}
</script>
<template>
  <section ref="root" class="downloads-page" aria-label="下载管理">
    <DownloadTabs
      :section="section"
      :directory="!!view?.directory"
      @select="changeSection"
      @open="model.open()"
      @settings="emit('dialog', { kind: 'settings' })"
    />
    <Transition name="download-section" mode="out-in">
      <div
        :key="section"
        id="download-content"
        class="download-content"
        role="tabpanel"
        :aria-labelledby="`download-tab-${section}`"
        :aria-busy="loading"
      >
        <DownloadToolbar
          :model="model"
          :section="section"
          :can-play="canPlay"
          :locked="locked"
          :preparing="!!preparing"
          @play="play()"
          @batch="openBatch"
          @clear="emit('dialog', { kind: 'clear' })"
          @settings="emit('dialog', { kind: 'settings' })"
        />
        <div v-if="error || readError" class="error-state" role="alert">
          {{ error || readError
          }}<button class="text-button" :disabled="loading" @click="model.refresh()">
            {{ loading ? '正在刷新…' : '刷新状态' }}</button
          ><button v-if="error" class="text-button" @click="error = ''">关闭提示</button>
        </div>
        <div v-if="notice" class="notice" role="status">
          {{ notice }}<button class="text-button" @click="notice = ''">关闭</button>
        </div>
        <DownloadOperations :model="model" />
        <div v-if="!view && loading" class="empty-state" role="status">
          <span class="spinner" />
          <p>正在读取下载任务…</p>
        </div>
        <div v-else-if="readError && !view" class="empty-state">
          <Icon name="download" />
          <p>暂时无法读取下载记录，请刷新重试。</p>
        </div>
        <div v-else-if="!tasks.length" class="empty-state">
          <Icon :name="section === 'mv' ? 'video' : 'music'" />
          <p>
            {{
              query.keyword
                ? '没有找到匹配的下载'
                : section === 'active'
                  ? '没有正在下载的任务'
                  : section === 'mv'
                    ? '还没有下载MV'
                    : section === 'voice'
                      ? '还没有下载声音'
                      : '还没有下载歌曲'
            }}
          </p>
          <p class="muted">在内容操作中选择下载，离线也能播放。</p>
        </div>
        <DownloadMvGrid
          v-else-if="section === 'mv'"
          :key="query.offset"
          :tasks="tasks"
          :more="model.hasMore.value"
          :loading-more="model.loadingMore.value"
          :more-error="model.moreError.value"
          @load="loadMore"
          @play="emit('playMv', $event)"
          @manage="emit('dialog', { kind: 'file', task: $event })"
        />
        <DownloadTable
          v-else
          :growth="columnGrowth"
          @columns="columnGrowth = $event"
          :tasks="tasks"
          :total="view?.total ?? 0"
          :query="query"
          :locked="locked"
          :playable="playable && !locked && !preparing"
          :current-id="currentId"
          :repairing="repairing"
          :more="model.hasMore.value"
          :loading-more="model.loadingMore.value"
          :more-error="model.moreError.value"
          @load="loadMore"
          @play="play"
          @manage="emit('dialog', { kind: 'file', task: $event })"
          @sort="filter(nextDownloadSort(query, $event), `[data-download-sort='${$event}']`)"
          @control="
            (task, action) => model.command({ kind: 'control', task_ids: [task.id], action })
          "
        />
      </div>
    </Transition>
  </section>
</template>
