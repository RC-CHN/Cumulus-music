<script setup lang="ts">
import { computed, nextTick, onMounted, ref, shallowRef } from 'vue';
import type { AccountView, PlaylistView } from '../../services/contracts';
import { failure } from '../../services/contracts';
import type { DownloadQuery } from '../../services/downloads';
import type { BatchCategory } from '../../services/downloadBatch';
import type { DownloadModel } from '../../composables/useDownloads';
import type { PlaylistManager } from '../../composables/usePlaylists';
import type { PlayerModel } from '../../composables/usePlayer';
import { useDownloadBatch } from '../../composables/useDownloadBatch';
import { useCore } from '../../services/core';
import DownloadBatchTable from './DownloadBatchTable.vue';
import DownloadBatchConfirm from './DownloadBatchConfirm.vue';
import PlaylistCollectDialog from '../PlaylistCollectDialog.vue';
import ModalTransition from '../ModalTransition.vue';
import Icon from '../Icon.vue';
import '../../styles/playlist-batch.css';
const props = defineProps<{
  account: AccountView;
  query: DownloadQuery;
  category: BatchCategory;
  model: DownloadModel;
  player: PlayerModel;
  manager: PlaylistManager;
  playlists: PlaylistView[];
}>();
const emit = defineEmits<{ close: []; expired: [] }>();
const batch = useDownloadBatch(
  useCore(),
  () => props.account,
  props.query,
  props.category,
  props.model,
  () => emit('expired'),
);
const { rows, chosen, selected, order, loading, error, migration } = batch;
const done = ref<HTMLButtonElement>(),
  dialog = ref<'collect' | 'delete'>(),
  confirmIds = shallowRef<string[]>([]),
  acting = ref(false),
  failed = ref(false),
  message = ref('');
const locked = computed(
  () =>
    acting.value ||
    loading.value ||
    props.model.busy.value ||
    !!props.model.view.value?.migration ||
    migration.value ||
    !!error.value,
);
const usable = computed(() => chosen.value.length > 0 && !locked.value);
const collectable = computed(() =>
  chosen.value.map((t) => t.track).filter((t) => !t.local && !t.missing),
);
onMounted(() => done.value?.focus({ preventScroll: true }));
function requestLeave(continuation?: () => void) {
  if (dialog.value || acting.value || props.manager.busy.value) return;
  if (continuation) continuation();
  else emit('close');
}
function escape(e: KeyboardEvent) {
  if ((e.target as Element)?.closest('dialog')) return;
  e.preventDefault();
  e.stopPropagation();
  requestLeave();
}
function fail(text: string) {
  message.value = text;
  failed.value = true;
}
async function reread() {
  await batch.read();
  if (batch.active() && !error.value) {
    await nextTick();
    done.value?.focus({ preventScroll: true });
  }
}
function withinLimit() {
  if (chosen.value.length <= 5000) return true;
  fail('单次最多处理 5000 项，请减少选择后重试。');
  return false;
}
async function play(queue: boolean) {
  if (!usable.value || !withinLimit()) return;
  const ids = chosen.value.map((t) => t.id);
  acting.value = true;
  failed.value = false;
  message.value = queue ? '正在添加所选下载…' : '正在准备播放…';
  try {
    const result = queue
      ? await props.player.queueDownloads(ids)
      : await props.player.downloads(ids, 0);
    if (!batch.active()) return;
    if (!queue && !result && props.player.error.value)
      throw { code: 'playback', message: props.player.error.value };
    message.value = result
      ? queue
        ? '已添加至播放列表'
        : '正在播放所选下载'
      : '播放操作已更新，本次操作已取消';
  } catch (e) {
    if (batch.active()) {
      const problem = failure(e);
      fail(problem.message);
      if (['profile_changed', 'session_expired'].includes(problem.code)) emit('expired');
    }
  } finally {
    if (batch.active()) acting.value = false;
  }
}
function askDelete() {
  if (!usable.value || !withinLimit()) return;
  confirmIds.value = chosen.value.map((t) => t.id);
  dialog.value = 'delete';
}
const confirmIssue = computed(() => {
  if (dialog.value !== 'delete' || acting.value) return '';
  if (migration.value || props.model.view.value?.migration)
    return '正在迁移下载目录，请稍后再删除。';
  if (error.value || loading.value || props.model.busy.value)
    return '下载状态尚未确认，请返回后重新读取。';
  const present = new Set(rows.value.map((t) => t.id));
  return confirmIds.value.every((id) => present.has(id))
    ? ''
    : '所选下载已改变，请返回后重新选择。';
});
async function remove() {
  if (
    acting.value ||
    dialog.value !== 'delete' ||
    !batch.active() ||
    locked.value ||
    confirmIssue.value
  )
    return;
  acting.value = true;
  failed.value = false;
  const ids = confirmIds.value;
  try {
    const result = await props.model.command({ kind: 'delete_files', task_ids: ids });
    if (!batch.active()) return;
    dialog.value = undefined;
    if (result) {
      if (result.failed.length)
        fail(
          '已删除 ' +
            result.accepted.length +
            ' 项，' +
            result.failed.length +
            ' 项未完成：' +
            result.failed[0]!.message,
        );
      else message.value = '已删除 ' + result.accepted.length + ' 项下载';
    } else fail(props.model.error.value || '未确认删除结果，请重新读取后核对。');
    await batch.read(true);
  } finally {
    if (batch.active()) {
      acting.value = false;
      await nextTick();
      if (!selected.value.size) done.value?.focus({ preventScroll: true });
    }
  }
}
defineExpose({ requestLeave });
</script>
<template>
  <section
    class="playlist-batch-page download-batch-page"
    :aria-label="'批量操作：已下载' + (category === 'song' ? '单曲' : '声音')"
    @keydown.esc="escape"
  >
    <header class="batch-toolbar">
      <div class="batch-buttons">
        <button
          class="primary batch-play"
          aria-label="播放所选下载"
          title="播放所选下载"
          :disabled="!usable || !player.ready.value || player.busy.value"
          @click="play(false)"
        >
          <Icon name="play" />
        </button>
        <button
          class="batch-queue"
          :title="player.view.value?.fm ? '私人漫游期间请先切换到普通播放队列' : undefined"
          :disabled="
            !usable ||
            !player.ready.value ||
            player.queueBusy.value ||
            player.busy.value ||
            player.view.value?.fm
          "
          @click="play(true)"
        >
          <Icon name="list" />添加到播放列表
        </button>
        <button
          v-if="category === 'song'"
          :disabled="!usable || !collectable.length"
          @click="dialog = 'collect'"
        >
          <Icon name="star" />收藏
        </button>
        <button :disabled="!usable" @click="askDelete"><Icon name="trash" />删除</button>
      </div>
      <button ref="done" class="batch-done" :disabled="acting" @click="requestLeave()">完成</button>
    </header>
    <p v-if="loading" class="batch-status" role="status">
      <span class="spinner" />正在读取全部下载…
    </p>
    <div v-if="error" class="error-state batch-error" role="alert">
      {{ error }}<button class="text-button" :disabled="loading" @click="reread">重新读取</button>
    </div>
    <p v-if="migration" class="batch-status" role="status">正在迁移下载目录，完成后可继续操作。</p>
    <div v-if="message" class="batch-operation-feedback" :class="{ failed }" role="status">
      {{ message
      }}<button v-if="failed" class="text-button" :disabled="acting || loading" @click="reread">
        重新读取
      </button>
    </div>
    <p v-if="!loading && !error && !rows.length" class="empty-state">
      {{ category === 'song' ? '没有已下载单曲' : '没有已下载声音' }}
    </p>
    <DownloadBatchTable
      v-if="rows.length"
      :category="category"
      :tasks="rows"
      :selected="selected"
      :disabled="locked"
      :order="order"
      @toggle="batch.toggle"
      @select="batch.select"
      @all="batch.all"
      @sort="batch.sort"
    />
    <ModalTransition :fallback-focus="() => done ?? null">
      <PlaylistCollectDialog
        v-if="dialog === 'collect'"
        :manager="manager"
        :epoch="account.epoch"
        :tracks="collectable"
        :playlists="playlists"
        @close="dialog = undefined"
        @expired="emit('expired')"
      />
      <DownloadBatchConfirm
        v-else-if="dialog === 'delete'"
        :count="confirmIds.length"
        :issue="confirmIssue"
        :busy="acting"
        @close="dialog = undefined"
        @confirm="remove"
      />
    </ModalTransition>
  </section>
</template>
