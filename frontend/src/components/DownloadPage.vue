<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import type { DownloadModel } from '../composables/useDownloads';
import type { DownloadDialogTarget, DownloadTask } from '../services/downloads';
import { bytesLabel, downloadStatus } from '../services/downloads';
import { qualities } from '../services/player';
import DownloadOperations from './DownloadOperations.vue';
import Icon from './Icon.vue';
const props = defineProps<{ model: DownloadModel; playable: boolean; currentId?: string }>();
const emit = defineEmits<{
  dialog: [target: DownloadDialogTarget];
  play: [ids: string[], index: number];
  playMv: [task: DownloadTask];
}>();
const { view, query, busy, loading, error, readError, notice } = props.model;
const keyword = ref(query.value.keyword),
  selected = ref(new Set<string>()),
  batch = ref(false);
const tasks = computed(() => view.value?.tasks ?? []);
const selectedTasks = computed(() => tasks.value.filter((t) => selected.value.has(t.id)));
watch(
  () => [query.value.tab, query.value.offset, query.value.keyword],
  () => {
    selected.value = new Set();
  },
);
watch(tasks, (rows) => {
  selected.value = new Set([...selected.value].filter((id) => rows.some((t) => t.id === id)));
});
function toggle(id: string) {
  const set = new Set(selected.value);
  set.has(id) ? set.delete(id) : set.add(id);
  selected.value = set;
}
function play(task?: DownloadTask) {
  if (task?.source === 'mv') {
    emit('playMv', task);
    return;
  }
  const rows = (task ? tasks.value : batch.value ? selectedTasks.value : tasks.value).filter(
    (t) => t.source !== 'mv',
  );
  if (rows.length)
    emit(
      'play',
      rows.map((t) => t.id),
      task ? rows.findIndex((t) => t.id === task.id) : 0,
    );
}
function quality(task: DownloadTask) {
  return qualities.find((q) => q.value === task.actualQuality)?.label ?? '音质待确认';
}
function time(ms: number | null) {
  return ms === null ? '时间未记录' : new Date(ms).toLocaleDateString();
}
const locked = computed(() => busy.value || !!view.value?.migration);
</script>
<template>
  <section class="downloads-page" aria-label="下载管理">
    <div class="download-heading">
      <h1>下载管理</h1>
      <div>
        <button class="text-button" :disabled="!view?.directory" @click="model.open()">
          打开目录</button
        ><button class="text-button" @click="emit('dialog', { kind: 'settings' })">下载设置</button>
      </div>
    </div>
    <div class="download-tabs" role="tablist" aria-label="下载分类">
      <button
        role="tab"
        :aria-selected="query.tab === 'completed'"
        :class="{ selected: query.tab === 'completed' }"
        @click="model.filter({ tab: 'completed', sort: 'completed', descending: true })"
      >
        已下载 <small>{{ view?.completedCount ?? 0 }}</small>
      </button>
      <button
        role="tab"
        :aria-selected="query.tab === 'active'"
        :class="{ selected: query.tab === 'active' }"
        @click="model.filter({ tab: 'active', sort: 'completed', descending: false })"
      >
        正在下载 <small>{{ view?.activeCount ?? 0 }}</small>
      </button>
    </div>
    <div class="download-toolbar">
      <template v-if="query.tab === 'active'">
        <button
          class="download-button"
          :disabled="locked || !view?.activeCount"
          @click="model.command({ kind: 'all', action: 'resume' })"
        >
          全部下载
        </button>
        <button
          class="download-button"
          :disabled="locked || !view?.activeCount"
          @click="model.command({ kind: 'all', action: 'pause' })"
        >
          全部暂停
        </button>
        <button
          class="download-button"
          :disabled="locked || !view?.activeCount"
          @click="emit('dialog', { kind: 'clear' })"
        >
          全部清空
        </button>
      </template>
      <template v-else>
        <button
          class="primary"
          :disabled="!playable || !(batch ? selectedTasks : tasks).some((t) => t.source !== 'mv')"
          @click="play()"
        >
          <Icon name="play" />{{
            batch ? '播放所选' : view && view.total > tasks.length ? '播放本页' : '播放全部'
          }}
        </button>
        <button
          class="download-button"
          :aria-pressed="batch"
          @click="
            batch = !batch;
            selected = new Set();
          "
        >
          {{ batch ? '取消批量' : '批量操作' }}
        </button>
        <label class="download-sort"
          >排序<select
            :value="query.sort"
            aria-label="下载排序"
            @change="
              model.filter({
                sort: ($event.target as HTMLSelectElement).value as typeof query.sort,
              })
            "
          >
            <option value="completed">完成时间</option>
            <option value="name">歌曲名称</option>
            <option value="artist">歌手</option>
            <option value="album">专辑</option>
            <option value="size">大小</option>
          </select></label
        >
        <button
          class="icon-button"
          :aria-label="query.descending ? '改为升序' : '改为降序'"
          @click="model.filter({ descending: !query.descending })"
        >
          <Icon :name="query.descending ? 'down' : 'up'" />
        </button>
      </template>
      <form class="download-search" @submit.prevent="model.filter({ keyword: keyword.trim() })">
        <Icon name="search" /><input
          v-model="keyword"
          placeholder="搜索下载的歌曲"
          aria-label="搜索下载的歌曲"
          maxlength="256"
        /><button
          v-if="keyword"
          type="button"
          class="icon-button"
          aria-label="清空下载搜索"
          @click="
            keyword = '';
            model.filter({ keyword: '' });
          "
        >
          <Icon name="close" />
        </button>
      </form>
    </div>
    <div v-if="error || readError" class="error-state" role="alert">
      {{ error || readError }}<button class="text-button" @click="model.refresh()">刷新状态</button
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
    <div v-else-if="!tasks.length" class="empty-state">
      <Icon name="music" />
      <p>
        {{
          query.keyword
            ? '没有找到匹配的下载'
            : query.tab === 'active'
              ? '没有正在下载的歌曲'
              : '还没有下载歌曲'
        }}
      </p>
      <p class="muted">在歌曲操作中选择下载，离线也能听。</p>
    </div>
    <div v-else class="download-table" role="table" :aria-rowcount="tasks.length + 1">
      <div
        class="download-row download-table-header"
        :class="{ active: query.tab === 'active' }"
        role="row"
      >
        <span role="columnheader"
          ><input
            v-if="batch && query.tab === 'completed'"
            type="checkbox"
            aria-label="选择本页全部下载"
            :checked="selectedTasks.length === tasks.length"
            @change="
              selected =
                selectedTasks.length === tasks.length ? new Set() : new Set(tasks.map((t) => t.id))
            "
          /><template v-else>#</template></span
        ><span role="columnheader">歌曲 / 歌手</span
        ><span role="columnheader">{{ query.tab === 'active' ? '下载进度' : '专辑' }}</span
        ><span v-if="query.tab === 'completed'" role="columnheader">完成时间</span
        ><span v-if="query.tab === 'completed'" role="columnheader">大小</span
        ><span role="columnheader" class="sr-only">操作</span>
      </div>
      <div class="download-scroll">
        <div
          v-for="(task, index) in tasks"
          :key="task.id"
          class="download-row"
          :class="{ active: query.tab === 'active', current: task.track.id === currentId }"
          role="row"
          @dblclick="query.tab === 'completed' && playable && play(task)"
        >
          <span role="cell"
            ><input
              v-if="batch && query.tab === 'completed'"
              type="checkbox"
              :aria-label="`选择 ${task.track.name}`"
              :checked="selected.has(task.id)"
              @change="toggle(task.id)"
            /><button
              v-else-if="query.tab === 'completed'"
              class="icon-button"
              :disabled="task.source !== 'mv' && !playable"
              :aria-label="`播放下载 ${task.track.name}`"
              @click="play(task)"
              @dblclick.stop
            >
              <Icon name="play" /></button
            ><span v-else class="muted">{{ query.offset + index + 1 }}</span></span
          >
          <div role="cell" class="download-track">
            <img
              v-if="task.track.coverUrl"
              :src="task.track.coverUrl"
              alt=""
              loading="lazy"
              referrerpolicy="no-referrer"
            /><span v-else class="track-art"><Icon name="music" /></span>
            <div>
              <span class="ellipsis" :title="task.track.name">{{ task.track.name }}</span
              ><small class="ellipsis"
                >{{ task.track.artists.join(' / ') || '歌手信息待获取' }}
                <span class="download-quality" v-if="task.source === 'cloud'">云盘原文件</span>
                <span class="download-quality" v-else-if="task.source === 'voice'">播客节目</span>
                <span class="download-quality" v-else-if="task.source === 'mv'"
                  >MV ·
                  {{ task.actualResolution ? `${task.actualResolution}P` : '清晰度待确认' }}</span
                >
                <span class="download-quality" v-else-if="task.actualQuality">{{
                  quality(task)
                }}</span
                ><span v-if="task.encrypted"> · NCM</span></small
              >
            </div>
          </div>
          <div v-if="query.tab === 'active'" role="cell" class="download-progress">
            <progress
              :value="task.progress ?? undefined"
              max="1"
              :aria-label="`${task.track.name} 下载进度`"
            /><small
              :class="{ 'form-warning': task.status === 'error' }"
              :title="downloadStatus(task)"
              >{{ downloadStatus(task) }}</small
            ><small>{{ bytesLabel(task.bytes) }} / {{ bytesLabel(task.total) }}</small>
          </div>
          <template v-else
            ><span role="cell" class="ellipsis" :title="task.track.album">{{
              task.track.album || '—'
            }}</span
            ><span role="cell" class="muted download-time">{{ time(task.completedMs) }}</span
            ><span role="cell" class="muted">{{ bytesLabel(task.total) }}</span></template
          >
          <div role="cell" class="download-row-actions" @dblclick.stop>
            <template v-if="query.tab === 'active'"
              ><button
                class="icon-button"
                :disabled="locked || !!task.fileOperation || task.status === 'cancelling'"
                :aria-label="`${['paused', 'error'].includes(task.status) ? '继续下载' : '暂停下载'} ${task.track.name}`"
                @click="
                  model.command({
                    kind: 'control',
                    task_ids: [task.id],
                    action: ['paused', 'error'].includes(task.status) ? 'resume' : 'pause',
                  })
                "
              >
                <Icon
                  :name="['paused', 'error'].includes(task.status) ? 'play' : 'pause'"
                /></button
              ><button
                class="icon-button"
                :disabled="locked || !!task.fileOperation"
                :aria-label="`取消下载 ${task.track.name}`"
                @click="model.command({ kind: 'control', task_ids: [task.id], action: 'cancel' })"
              >
                <Icon name="close" /></button
            ></template>
            <template v-else
              ><button
                v-if="task.cleanupPending"
                class="text-button download-repair"
                :disabled="locked || view?.jobs.some((j) => j.id === task.id && j.running)"
                @click="model.command({ kind: 'control', task_ids: [task.id], action: 'repair' })"
              >
                修复</button
              ><button
                class="icon-button"
                :aria-label="`管理下载 ${task.track.name}`"
                @click="emit('dialog', { kind: 'file', task })"
              >
                ···
              </button></template
            >
          </div>
        </div>
      </div>
    </div>
    <footer v-if="view && view.total > query.limit" class="download-pagination">
      <button
        class="text-button"
        :disabled="!query.offset || loading"
        @click="model.filter({ offset: query.offset - query.limit })"
      >
        上一页</button
      ><span
        >{{ Math.floor(query.offset / query.limit) + 1 }} /
        {{ Math.ceil(view.total / query.limit) }} · {{ view.total }} 首</span
      ><button
        class="text-button"
        :disabled="query.offset + query.limit >= view.total || loading"
        @click="model.filter({ offset: query.offset + query.limit })"
      >
        下一页
      </button>
    </footer>
  </section>
</template>
