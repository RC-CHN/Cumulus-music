<script setup lang="ts">
import { computed, nextTick, onMounted, ref } from 'vue';
import type { AccountView, PlaylistView } from '../../services/contracts';
import { failure } from '../../services/contracts';
import { useCore } from '../../services/core';
import type { CloudModel } from '../../composables/useCloud';
import type { PlayerModel } from '../../composables/usePlayer';
import type { DownloadModel } from '../../composables/useDownloads';
import type { PlaylistManager } from '../../composables/usePlaylists';
import { useCloudBatch } from '../../composables/useCloudBatch';
import CloudBatchTable from './CloudBatchTable.vue';
import CloudDeleteDialog from '../CloudDeleteDialog.vue';
import PlaylistCollectDialog from '../PlaylistCollectDialog.vue';
import DownloadDialog from '../DownloadDialog.vue';
import ModalTransition from '../ModalTransition.vue';
import Icon from '../Icon.vue';
import '../../styles/playlist-batch.css';
import '../../styles/cloud-batch.css';
const props = defineProps<{
  account: AccountView;
  model: CloudModel;
  player: PlayerModel;
  downloads: DownloadModel;
  manager: PlaylistManager;
  playlists: PlaylistView[];
  leaving: boolean;
}>();
const emit = defineEmits<{ close: []; expired: []; downloaded: [] }>();
const batch = useCloudBatch(
  useCore(),
  () => ({ ...props.account, epoch: props.model.accountEpoch.value }),
  () => emit('expired'),
);
const { data, tracks, chosen, selected, loading, error, progress } = batch;
const retiring = ref(false);
const dialog = ref<'collect' | 'download' | 'delete'>(),
  acting = ref(false),
  message = ref(''),
  failed = ref(false),
  done = ref<HTMLButtonElement>();
const locked = computed(
  () =>
    retiring.value ||
    !batch.active() ||
    loading.value ||
    acting.value ||
    !!dialog.value ||
    props.leaving ||
    props.manager.busy.value ||
    props.model.busy.value,
);
const stale = computed(
  () => !!data.value && props.model.view.value?.revision !== data.value.revision,
);
const usable = computed(
  () => batch.active() && !!chosen.value.length && !locked.value && !error.value && !stale.value,
);
const cloudTracks = computed(() => chosen.value.map((v) => v.track));
const deletion = computed(() => ({
  epoch: batch.owner,
  ids: chosen.value.map((v) => v.track.id),
  titles: chosen.value.map((v) => v.track.name),
  revision: data.value?.revision ?? '',
}));
async function reload() {
  await batch.load();
  if (batch.active() && !error.value) {
    await props.model.refresh(false, true);
    if (batch.active() && !stale.value) {
      props.model.readError.value = '';
      message.value = '';
      failed.value = false;
    }
  }
}
onMounted(() => {
  done.value?.focus();
  void reload();
});
function requestLeave(continuation?: () => void) {
  if (
    dialog.value ||
    acting.value ||
    props.leaving ||
    props.manager.busy.value ||
    props.model.busy.value
  )
    return;
  if (retiring.value) return;
  retiring.value = true;
  if (continuation) continuation();
  else emit('close');
}
function escape(event: KeyboardEvent) {
  if ((event.target as HTMLElement).closest('dialog')) return;
  event.preventDefault();
  event.stopPropagation();
  requestLeave();
}
async function perform(kind: 'play' | 'queue') {
  if (!usable.value || !data.value) return;
  if (chosen.value.length > 10000) {
    message.value = '单次播放最多选择 10000 首歌曲，请减少选择后重试。';
    failed.value = true;
    return;
  }
  const origin = document.activeElement instanceof HTMLElement ? document.activeElement : null;
  acting.value = true;
  failed.value = false;
  message.value = kind === 'play' ? '正在准备播放…' : '正在更新播放列表…';
  try {
    const ids = chosen.value.map((v) => v.track.id),
      revision = data.value.revision;
    const value =
      kind === 'play'
        ? await props.player.selectForPage({
            kind: 'cloud_batch',
            ids,
            revision,
            quality: props.player.view.value?.qualityPreferences.quality ?? 'standard',
          })
        : await props.player.queueCloud(ids, revision);
    if (!batch.active()) return;
    message.value = value
      ? kind === 'play'
        ? '正在播放所选云盘歌曲'
        : '播放列表已更新'
      : '本次播放操作已取消';
  } catch (e) {
    if (batch.active()) {
      const issue = failure(e);
      message.value = issue.message;
      failed.value = true;
      if (['session_expired', 'profile_changed'].includes(issue.code)) emit('expired');
    }
  } finally {
    if (batch.active()) {
      acting.value = false;
      await nextTick();
      if (
        !retiring.value &&
        !props.leaving &&
        origin?.closest('.cloud-batch-page') &&
        (document.activeElement === document.body || document.activeElement === origin)
      ) {
        const target = origin.isConnected && !origin.matches(':disabled') ? origin : done.value;
        target?.focus({ preventScroll: true });
      }
    }
  }
}
function chooseDialog(kind: 'collect' | 'download' | 'delete') {
  if (!usable.value) return;
  if (kind !== 'collect' && chosen.value.length > 5000) {
    message.value = `单次${kind === 'download' ? '下载' : '删除'}最多选择 5000 首云盘歌曲，请减少选择后重试。`;
    failed.value = true;
    return;
  }
  dialog.value = kind;
}
async function deleted() {
  dialog.value = undefined;
  if (batch.active() && stale.value) await reload();
}
async function reread() {
  const hadFocus = document.activeElement?.closest('.cloud-batch-feedback,.batch-error');
  await reload();
  if (batch.active() && hadFocus) {
    await nextTick();
    done.value?.focus({ preventScroll: true });
  }
}
defineExpose({ requestLeave });
</script>
<template>
  <section
    class="playlist-batch-page cloud-batch-page"
    aria-label="批量操作：音乐云盘"
    :data-cloud-batch-loading="loading"
    @keydown.esc="escape"
  >
    <header class="batch-toolbar">
      <div class="batch-buttons">
        <button
          class="primary batch-play"
          aria-label="播放所选云盘歌曲"
          title="播放所选云盘歌曲"
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
          :title="player.view.value?.fm ? '请先退出私人漫游再添加到普通播放列表' : undefined"
          @click="perform('queue')"
        >
          <Icon name="list" />添加到播放列表
        </button>
        <button :disabled="!usable || downloads.busy.value" @click="chooseDialog('download')">
          <Icon name="download" />下载
        </button>
        <button :disabled="!usable" @click="chooseDialog('collect')">
          <Icon name="star" />收藏
        </button>
        <button :disabled="!usable" @click="chooseDialog('delete')">
          <Icon name="trash" />删除
        </button>
      </div>
      <button
        ref="done"
        class="batch-done"
        :disabled="acting || leaving || manager.busy.value || model.busy.value"
        @click="requestLeave()"
      >
        {{ leaving ? '正在返回…' : '完成' }}
      </button>
    </header>
    <p class="batch-status muted">
      音乐云盘 · 全部歌曲<template v-if="data">（{{ data.total }} 首）</template>
    </p>
    <p v-if="loading" class="batch-status" role="status"><span class="spinner" />{{ progress }}</p>
    <div v-if="error" class="error-state batch-error" role="alert">
      {{ error }}<button class="text-button" :disabled="loading" @click="reread">重新读取</button>
    </div>
    <div
      v-else-if="stale && !loading"
      class="batch-operation-feedback failed cloud-batch-feedback"
      role="status"
    >
      云盘列表已变化，请重新读取后操作。<button class="text-button" @click="reread">
        重新读取
      </button>
    </div>
    <div
      v-if="message"
      class="batch-operation-feedback cloud-batch-feedback"
      :class="{ failed }"
      role="status"
    >
      {{ message
      }}<button v-if="failed" class="text-button" :disabled="locked" @click="reread">
        重新读取
      </button>
    </div>
    <p v-if="!loading && !error && !tracks.length" class="empty-state">云盘中还没有歌曲</p>
    <CloudBatchTable
      v-if="tracks.length"
      :tracks="tracks"
      :selected="selected"
      :disabled="locked || !!error || stale"
      :current-id="
        player.view.value?.resourceKind === 'cloud' ? player.view.value?.track?.id : undefined
      "
      @toggle="batch.toggle"
      @select="batch.select"
      @all="batch.all"
    />
    <ModalTransition
      :restore-focus="account.epoch === batch.owner"
      :fallback-focus="() => done ?? null"
    >
      <PlaylistCollectDialog
        v-if="dialog === 'collect'"
        :manager="manager"
        :epoch="batch.owner"
        :tracks="cloudTracks"
        :playlists="playlists"
        @close="dialog = undefined"
        @expired="emit('expired')"
      />
      <DownloadDialog
        v-else-if="dialog === 'download'"
        :model="downloads"
        :target="{ kind: 'cloud', tracks: cloudTracks, revision: data?.revision ?? '' }"
        @close="dialog = undefined"
        @started="emit('downloaded')"
      />
      <CloudDeleteDialog
        v-else-if="dialog === 'delete'"
        :model="model"
        :target="deletion"
        @close="deleted"
      />
    </ModalTransition>
  </section>
</template>
