<script setup lang="ts">
import { computed, nextTick, onMounted, onBeforeUnmount, ref } from 'vue';
import { failure, type AccountView, type PlaylistView } from '../../services/contracts';
import type { PlaylistManager } from '../../composables/usePlaylists';
import type { PlayerModel } from '../../composables/usePlayer';
import type { DownloadModel } from '../../composables/useDownloads';
import { usePlaylistBatch } from '../../composables/usePlaylistBatch';
import { useCore } from '../../services/core';
import PlaylistBatchTable from './PlaylistBatchTable.vue';
import PlaylistMutationDialog from './PlaylistMutationDialog.vue';
import PlaylistCollectDialog from '../PlaylistCollectDialog.vue';
import DownloadDialog from '../DownloadDialog.vue';
import ModalTransition from '../ModalTransition.vue';
import Icon from '../Icon.vue';
import '../../styles/playlist-batch.css';
const props = defineProps<{
  playlist: PlaylistView;
  account: AccountView;
  manager: PlaylistManager;
  player: PlayerModel;
  downloads: DownloadModel;
  playlists: PlaylistView[];
  liked: Set<string>;
  leaving: boolean;
}>();
const emit = defineEmits<{ close: []; expired: []; changed: []; downloaded: [] }>();
const batch = usePlaylistBatch(
  useCore(),
  () => props.account,
  props.playlist.id,
  () => emit('expired'),
);
const { data, tracks, chosen, available, selected, loading, error, progress } = batch;
const dialog = ref<'collect' | 'download' | 'delete'>(),
  message = ref(''),
  failed = ref(false),
  acting = ref(false);
const done = ref<HTMLButtonElement>(),
  retry = ref<HTMLButtonElement>();
const locked = computed(
  () => loading.value || acting.value || props.manager.busy.value || props.leaving,
);
const usable = computed(() => available.value.length > 0 && !locked.value && !error.value);
const canDelete = computed(
  () => props.playlist.owned && !props.playlist.favorite && !tracks.value.some((t) => t.local),
);
let alive = true,
  retryAction: (() => Promise<void>) | undefined;
onMounted(() => done.value?.focus());
onBeforeUnmount(() => {
  alive = false;
});
function requestLeave(continuation?: () => void) {
  if (props.leaving || dialog.value) return;
  if (props.manager.busy.value) {
    message.value = '正在保存，请稍候…';
    return;
  }
  if (continuation) continuation();
  else emit('close');
}
function escape(event: KeyboardEvent) {
  // Native dialogs own Escape until their closing transition has finished.
  if ((event.target as HTMLElement)?.closest('dialog')) return;
  event.preventDefault();
  event.stopPropagation();
  requestLeave();
}
async function reload() {
  const focused =
    document.activeElement === retry.value ||
    document.activeElement?.matches('[data-batch-reread]');
  await batch.load();
  if (alive && !error.value) {
    failed.value = false;
    message.value = '';
    retryAction = undefined;
  }
  if (alive && focused && !error.value) {
    await nextTick();
    done.value?.focus();
  }
}
async function perform(kind: 'play' | 'queue') {
  if (!usable.value || !data.value) return;
  if (available.value.length > 10000) {
    message.value = '单次播放最多选择 10000 首歌曲，请减少选择后重试。';
    failed.value = true;
    return;
  }
  const selection = {
    id: props.playlist.id,
    ids: available.value.map((t) => t.id),
    revision: data.value.revision,
    quality: props.player.view.value?.requestedQuality ?? 'standard',
  };
  const wasRetry = !!document.activeElement?.closest('.batch-operation-feedback');
  acting.value = true;
  failed.value = false;
  message.value = kind === 'play' ? '正在准备播放…' : '正在添加所选歌曲…';
  retryAction = () => perform(kind);
  try {
    const value =
      kind === 'play'
        ? await props.player.select({ kind: 'playlist_batch', ...selection })
        : await props.player.queuePlaylist(selection.id, selection);
    if (!alive) return;
    if (!value && props.player.error.value) throw { message: props.player.error.value };
    message.value = value
      ? kind === 'play'
        ? '正在播放所选歌曲'
        : '已添加至播放列表'
      : '播放操作已更新，本次操作已取消';
    if (wasRetry) {
      await nextTick();
      done.value?.focus();
    }
  } catch (value) {
    if (!alive) return;
    const issue = failure(value);
    message.value = issue.message;
    failed.value = true;
    if (['session_expired', 'profile_changed'].includes(issue.code)) emit('expired');
  } finally {
    if (alive) acting.value = false;
  }
}
async function removed() {
  emit('changed');
  await batch.load();
  if (alive) message.value = '已删除所选歌曲';
}
defineExpose({ requestLeave });
</script>
<template>
  <section
    class="playlist-batch-page"
    :aria-label="`批量操作：${playlist.name}`"
    @keydown.esc="escape"
  >
    <header class="batch-toolbar">
      <div class="batch-buttons">
        <button
          class="primary batch-play"
          aria-label="播放所选歌曲"
          title="播放所选歌曲"
          :disabled="!usable || !player.ready.value || player.busy.value"
          @click="perform('play')"
        >
          <Icon name="play" />
        </button>
        <button
          class="batch-queue"
          :disabled="
            !usable ||
            !player.ready.value ||
            player.busy.value ||
            player.queueBusy.value ||
            player.view.value?.fm
          "
          :title="player.view.value?.fm ? '私人漫游期间请先切换到普通播放队列' : undefined"
          @click="perform('queue')"
        >
          <Icon name="list" />添加到播放列表
        </button>
        <button :disabled="!usable || downloads.busy.value" @click="dialog = 'download'">
          <Icon name="download" />下载
        </button>
        <button :disabled="!usable" @click="dialog = 'collect'"><Icon name="star" />收藏</button>
        <button
          v-if="playlist.owned"
          :disabled="!canDelete || !chosen.length || locked || !!error"
          :title="!canDelete ? '暂不支持批量删除喜欢列表或含本地歌曲的歌单' : undefined"
          @click="dialog = 'delete'"
        >
          <Icon name="trash" />删除
        </button>
      </div>
      <button
        ref="done"
        class="batch-done"
        :disabled="manager.busy.value || leaving"
        @click="requestLeave()"
      >
        {{ leaving ? '正在返回…' : '完成' }}
      </button>
    </header>
    <p v-if="loading" class="batch-status" role="status"><span class="spinner" />{{ progress }}</p>
    <div v-if="error" class="error-state batch-error" role="alert">
      <span>{{ error }}</span
      ><button ref="retry" class="text-button" :aria-disabled="loading" @click="reload">
        {{ loading ? '正在读取…' : '重新读取' }}
      </button>
    </div>
    <p v-if="chosen.length > available.length" class="batch-status muted">
      选中的
      {{ chosen.length - available.length }} 首本地或不可用歌曲暂不能用于在线播放、下载和收藏。
    </p>
    <div v-if="message" class="batch-operation-feedback" :class="{ failed }" role="status">
      {{ message
      }}<button
        v-if="failed && retryAction"
        class="text-button"
        :aria-disabled="acting"
        @click="retryAction?.()"
      >
        重试
      </button>
      <button
        v-if="failed"
        class="text-button"
        data-batch-reread
        :aria-disabled="locked"
        @click="!locked && reload()"
      >
        重新读取
      </button>
    </div>
    <p v-if="!loading && !error && !tracks.length" class="empty-state">歌单中还没有歌曲</p>
    <PlaylistBatchTable
      v-if="tracks.length"
      :tracks="tracks"
      :selected="selected"
      :liked="liked"
      :disabled="locked || !!error"
      :current-id="player.view.value?.track?.id"
      @toggle="batch.toggle"
      @select="batch.select"
      @all="batch.all"
    />
    <ModalTransition :fallback-focus="() => done ?? null">
      <PlaylistCollectDialog
        v-if="dialog === 'collect'"
        :manager="manager"
        :epoch="account.epoch"
        :tracks="available"
        :playlists="playlists"
        @close="dialog = undefined"
        @expired="emit('expired')"
      />
      <DownloadDialog
        v-else-if="dialog === 'download'"
        :model="downloads"
        :target="{ kind: 'start', tracks: available }"
        @close="dialog = undefined"
        @started="emit('downloaded')"
      />
      <PlaylistMutationDialog
        v-else-if="dialog === 'delete'"
        :playlist="data?.playlist ?? playlist"
        :tracks="chosen"
        :manager="manager"
        @close="dialog = undefined"
        @changed="removed"
      />
    </ModalTransition>
  </section>
</template>
