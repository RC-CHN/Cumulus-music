<script setup lang="ts">
import { computed, nextTick, onMounted, ref } from 'vue';
import type { PlaylistManager } from '../composables/usePlaylists';
import type { PlaylistView, TrackView } from '../services/contracts';
import { coverMessage } from '../services/playlistCover';
import {
  playlistLength,
  playlistPending,
  type PlaylistMutationView,
  type PlaylistDialogTarget,
} from '../services/playlists';
import Icon from './Icon.vue';
const props = defineProps<{
  manager: PlaylistManager;
  target: PlaylistDialogTarget;
  playlists: PlaylistView[];
}>();
const emit = defineEmits<{
  close: [];
  open: [id: string];
  download: [track: TrackView];
  comments: [track: TrackView];
  recoverCover: [id: string];
}>();
const { busy, error, result, operations, coverOperations } = props.manager;
const dialog = ref<HTMLDialogElement>(),
  nameInput = ref<HTMLInputElement>();
const name = ref(''),
  privateList = ref(true),
  destination = ref(''),
  link = ref(''),
  localError = ref('');
const title = computed(() =>
  props.target.kind === 'create'
    ? '新建歌单'
    : props.target.kind === 'history'
      ? '歌单操作记录'
      : props.target.kind === 'open'
        ? '打开歌单'
        : '歌曲操作',
);
const own = computed(() => props.playlists.filter((p) => p.owned && !p.favorite));
const pending = computed(() => result.value && playlistPending(result.value.phase));
const invalid = computed(
  () => !name.value.trim() || /[@#]/.test(name.value) || playlistLength(name.value.trim()) > 40,
);
onMounted(() => {
  dialog.value?.showModal();
  props.manager.reset();
  destination.value = own.value[0]?.id ?? '';
  if (props.target.kind === 'create') void nextTick(() => nameInput.value?.focus());
  if (props.target.kind === 'history') void props.manager.refreshOperations();
});
// New surfaces reset their scope on mount; an outgoing dialog must not reset
// the editor page which replaces it during the shared leave animation.
function close() {
  if (!busy.value) emit('close');
}
function finish(value: PlaylistMutationView | undefined) {
  if (value && props.manager.saved(value.phase)) emit('close');
}
async function submit() {
  localError.value = '';
  if (props.target.kind === 'open') {
    let id = link.value.trim();
    if (!/^[1-9][0-9]*$/.test(id))
      try {
        const u = new URL(id);
        if (u.hostname !== 'music.163.com') throw new Error();
        id =
          new URLSearchParams((u.hash.includes('?') ? u.hash.split('?')[1] : u.search) || '').get(
            'id',
          ) ?? '';
      } catch {
        id = '';
      }
    if (!/^[1-9][0-9]*$/.test(id)) {
      localError.value = '请输入网易云音乐歌单分享链接';
      return;
    }
    emit('open', id);
    emit('close');
    return;
  }
  if (props.target.kind === 'create' && !invalid.value)
    finish(
      await props.manager.mutate({ kind: 'create', name: name.value, private: privateList.value }),
    );
}
async function trackAction(action: 'add' | 'delete' | 'up' | 'down') {
  if (props.target.kind !== 'track') return;
  const id = action === 'add' ? destination.value : props.target.playlist?.id;
  if (id) finish(await props.manager.tracks(id, action, props.target.track.id));
}
</script>
<template>
  <dialog
    ref="dialog"
    class="playlist-dialog"
    aria-labelledby="playlist-dialog-title"
    @cancel.prevent="close"
  >
    <header>
      <h2 id="playlist-dialog-title">{{ title }}</h2>
      <button class="icon-button" aria-label="关闭歌单操作" :disabled="busy" @click="close">
        <Icon name="close" />
      </button>
    </header>
    <p v-if="error || localError" class="error-state" role="alert">{{ localError || error }}</p>
    <div v-if="result" class="mutation-result" :data-phase="result.phase" role="status">
      {{ result.message
      }}<button
        v-if="playlistPending(result.phase)"
        class="text-button"
        :disabled="busy"
        @click="manager.reconcile(result.operationId)"
      >
        核对结果
      </button>
    </div>
    <div v-if="target.kind === 'history'" class="operation-list">
      <p class="muted">未确认的操作仅核对实际结果，避免重复提交。</p>
      <p v-if="!operations.length && !coverOperations.length" class="empty-state">
        没有待处理的歌单操作
      </p>
      <article v-for="op in coverOperations" :key="op.id">
        <strong>{{ playlists.find((p) => p.id === op.playlistId)?.name ?? '歌单' }} · 封面</strong>
        <p>{{ coverMessage(op) }}</p>
        <button class="text-button" @click="emit('recoverCover', op.playlistId)">处理封面</button>
      </article>
      <article v-for="op in operations" :key="op.operationId">
        <strong>{{ op.label || '歌单操作' }}</strong>
        <p>{{ op.message }}</p>
        <button
          v-if="playlistPending(op.phase)"
          class="text-button"
          :disabled="busy"
          @click="manager.reconcile(op.operationId)"
        >
          核对结果
        </button>
        <button
          v-else-if="op.playlistId"
          class="text-button"
          @click="
            emit('open', op.playlistId);
            emit('close');
          "
        >
          查看歌单
        </button>
      </article>
      <button class="text-button" :disabled="busy" @click="manager.refreshOperations">
        刷新记录
      </button>
    </div>
    <div v-else-if="target.kind === 'track'" class="track-menu">
      <button
        v-if="!target.track.local"
        class="text-button"
        @click="emit('comments', target.track)"
      >
        查看歌曲评论
      </button>
      <h3>{{ target.track.name }}</h3>
      <p class="muted">{{ target.track.artists.join(' / ') }}</p>
      <button
        class="download-button"
        :disabled="busy || target.track.missing || target.track.local"
        @click="emit('download', target.track)"
      >
        下载歌曲
      </button>
      <label
        >收藏到歌单<select
          v-model="destination"
          aria-label="选择目标歌单"
          :disabled="busy || !!pending"
        >
          <option v-for="p in own" :key="p.id" :value="p.id">{{ p.name }}</option>
        </select></label
      >
      <button
        class="primary"
        :disabled="!destination || busy || !!pending || target.track.local || target.track.missing"
        @click="trackAction('add')"
      >
        添加到歌单
      </button>
      <div v-if="target.playlist?.owned && !target.playlist.favorite" class="track-menu-actions">
        <p class="muted">当前歌单：{{ target.playlist.name }}</p>
        <button class="text-button" :disabled="busy || !!pending" @click="trackAction('up')">
          上移一首
        </button>
        <button class="text-button" :disabled="busy || !!pending" @click="trackAction('down')">
          下移一首
        </button>
        <button
          class="danger text-button"
          :disabled="busy || !!pending"
          @click="trackAction('delete')"
        >
          从当前歌单移除
        </button>
      </div>
    </div>
    <form v-else @submit.prevent="submit">
      <fieldset :disabled="busy || !!pending">
        <label v-if="target.kind === 'open'"
          >歌单分享链接<input
            v-model="link"
            aria-label="歌单分享链接"
            placeholder="粘贴网易云音乐歌单链接"
        /></label>
        <template v-else>
          <label
            >歌单名称<input
              ref="nameInput"
              v-model="name"
              aria-label="歌单名称"
              aria-describedby="playlist-name-count"
              :aria-invalid="invalid"
              placeholder="输入歌单标题"
              maxlength="80"
            /><small
              id="playlist-name-count"
              :class="{ 'form-warning': playlistLength(name.trim()) > 40 }"
              >{{ playlistLength(name.trim()) }} / 40 · 中文计 2 个字符</small
            ></label
          >
          <label class="private-checkbox"
            ><input
              v-model="privateList"
              type="checkbox"
              aria-label="设为私密歌单"
            />设为私密歌单</label
          >
          <p v-if="!privateList" class="muted">公开歌单可被其他用户查看和收藏。</p>
          <p v-if="name && invalid" class="form-warning">
            名称不能为空、不能包含 @ 或 #，并请保持名称在字数上限内。
          </p>
        </template>
        <footer>
          <button type="submit" class="primary" :disabled="target.kind !== 'open' && invalid">
            {{ busy ? '正在保存…' : target.kind === 'open' ? '打开' : '创建歌单' }}
          </button>
        </footer>
      </fieldset>
    </form>
    <p v-if="busy" class="muted" role="status">正在提交并核对，请稍候…</p>
  </dialog>
</template>
