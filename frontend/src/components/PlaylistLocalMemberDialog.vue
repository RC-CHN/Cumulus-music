<script setup lang="ts">
import { computed, nextTick, onMounted, ref } from 'vue';
import type { PlaylistLocalReference, PlaylistView } from '../services/contracts';
import { useCore } from '../services/core';
import Icon from './Icon.vue';

type Target =
  | { kind: 'add'; title: string; row: PlaylistLocalReference }
  | {
      kind: 'remove';
      title: string;
      row: PlaylistLocalReference;
      playlist: PlaylistView;
      revision: string;
    };

const props = defineProps<{
  epoch: string;
  target: Target;
  playlists: PlaylistView[];
  live: () => boolean;
}>();
const emit = defineEmits<{ close: []; done: [id: string, changed: boolean] }>();
const core = useCore();
const dialog = ref<HTMLDialogElement>();
const cancel = ref<HTMLButtonElement>();
const destination = ref('');
const busy = ref(false);
const error = ref('');
const owned = computed(() => props.playlists.filter((list) => list.owned && !list.favorite));
onMounted(() => {
  destination.value = owned.value[0]?.id ?? '';
  dialog.value?.showModal();
  void nextTick(() => cancel.value?.focus({ preventScroll: true }));
});
function close() {
  if (!busy.value) emit('close');
}
async function submit() {
  if (busy.value || !props.live()) return;
  const { epoch, target } = props;
  const id = target.kind === 'remove' ? target.playlist.id : destination.value;
  if (!id || (target.kind === 'add' && !owned.value.some((row) => row.id === id))) return;
  busy.value = true;
  error.value = '';
  try {
    let revision: string;
    if (target.kind === 'add') {
      const current = await core.playlistPage(id, { offset: 0, limit: 1 });
      if (!props.live() || current.epoch !== epoch) return;
      if (current.value.playlistOffline)
        throw { code: 'playlist_offline', message: '请联网后再编辑歌单成员' };
      if (current.value.playlist?.id !== id || !current.value.playlistLocal)
        throw new Error('歌单不可编辑');
      revision = current.value.playlistLocal.revision;
    } else revision = target.revision;
    if (!props.live()) return;
    const reply = await core.mutatePlaylistLocal(epoch, id, revision, {
      op: target.kind,
      rows: [target.row],
    });
    if (props.live() && reply.epoch === epoch) emit('done', id, reply.value.changed);
  } catch {
    if (props.live()) error.value = '无法更新歌单。请刷新歌单或重新扫描本地音乐后重试。';
  } finally {
    busy.value = false;
  }
}
</script>

<template>
  <dialog
    ref="dialog"
    class="playlist-dialog"
    aria-labelledby="playlist-local-member-title"
    @cancel.prevent="close"
  >
    <header>
      <h2 id="playlist-local-member-title">
        {{ target.kind === 'add' ? '添加到歌单' : '从歌单中移除' }}
      </h2>
      <button class="icon-button" aria-label="关闭歌单操作" :disabled="busy" @click="close">
        <Icon name="close" />
      </button>
    </header>
    <form class="track-menu" @submit.prevent="submit">
      <p v-if="target.kind === 'remove'">
        将「{{ target.title }}」从「{{ target.playlist.name }}」中移除？
      </p>
      <p v-else>将「{{ target.title }}」添加到自建歌单。</p>
      <p class="muted">
        {{ target.kind === 'add' ? '本地歌曲只在此设备上显示。' : '移除不会删除本地文件。' }}
      </p>
      <label v-if="target.kind === 'add' && owned.length">
        选择歌单
        <select v-model="destination" aria-label="选择本地曲目目标歌单" :disabled="busy">
          <option v-for="list in owned" :key="list.id" :value="list.id">{{ list.name }}</option>
        </select>
      </label>
      <p v-else-if="target.kind === 'add'" class="empty-state">没有可编辑的自建歌单</p>
      <p v-if="error" class="error-state" role="alert">{{ error }}</p>
      <div class="track-remove-actions">
        <button ref="cancel" type="button" class="secondary" :disabled="busy" @click="close">
          取消
        </button>
        <button
          type="submit"
          class="primary"
          :disabled="busy || (target.kind === 'add' && !destination)"
        >
          {{ busy ? '正在保存…' : target.kind === 'add' ? '添加' : '移除' }}
        </button>
      </div>
    </form>
  </dialog>
</template>
