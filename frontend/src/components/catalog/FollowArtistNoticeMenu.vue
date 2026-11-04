<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, watch } from 'vue';
import ActionMenu from '../ActionMenu.vue';
import { failure } from '../../services/contracts';
import { useCore } from '../../services/core';
import type { ReleaseNotificationState } from '../../services/followNewSong';

const props = defineProps<{ epoch: string; artistId: string; artistName: string }>();
const emit = defineEmits<{ muted: [id: string]; expired: [] }>();
const core = useCore();
const menu = [{ key: 'disable', label: '不接收此歌手的通知' }];
const root = ref<HTMLElement>();
const dialog = ref<HTMLDialogElement>();
const closeButton = ref<HTMLButtonElement>();
const open = ref(false);
const busy = ref(false);
const state = ref<ReleaseNotificationState>();
const error = ref('');
const title = computed(() =>
  state.value === 'uncertain'
    ? '新歌通知状态待核对'
    : state.value === 'confirmed'
      ? '已取消该歌手的新歌通知'
      : '确认取消该歌手新歌通知？',
);
let generation = 0;

function retire() {
  ++generation;
  open.value = false;
  busy.value = false;
  state.value = undefined;
  error.value = '';
}
watch(() => [props.epoch, props.artistId] as const, retire, { flush: 'sync' });
onBeforeUnmount(retire);

async function request(action: 'inspect' | 'disable') {
  if (busy.value || (action === 'disable' && (!open.value || state.value !== 'ready'))) return;
  const epoch = props.epoch;
  const id = props.artistId;
  const requestId = ++generation;
  busy.value = true;
  error.value = '';
  open.value = true;
  if (action === 'inspect') state.value = undefined;
  await nextTick();
  if (requestId !== generation) return;
  if (dialog.value && !dialog.value.open) dialog.value.showModal();
  try {
    const reply = await core.releaseNotification(epoch, id, action);
    if (reply.epoch !== epoch || requestId !== generation) return;
    state.value = reply.value;
    if (reply.value === 'confirmed' && action === 'disable') {
      open.value = false;
      emit('muted', id);
    }
  } catch (reason) {
    if (requestId !== generation) return;
    const issue = failure(reason);
    error.value = issue.message;
    if (issue.code === 'session_expired' || issue.code === 'profile_changed') emit('expired');
  } finally {
    if (requestId === generation) {
      busy.value = false;
      await nextTick();
      if (requestId === generation && open.value) closeButton.value?.focus({ preventScroll: true });
    }
  }
}

async function close() {
  if (busy.value) return;
  retire();
  await nextTick();
  root.value?.querySelector<HTMLElement>('.menu-trigger')?.focus({ preventScroll: true });
}
</script>

<template>
  <div ref="root" class="follow-artist-notice">
    <ActionMenu
      :label="`${artistName} 更多操作`"
      :items="menu"
      variant="song"
      @select="request('inspect')"
    />
    <dialog
      v-if="open"
      ref="dialog"
      class="playlist-dialog playlist-editor-confirm"
      :aria-label="title"
      @cancel.prevent="close"
    >
      <header>
        <h2>{{ title }}</h2>
      </header>
      <p v-if="state === 'uncertain'">
        上一次操作未收到确认。请在官方客户端核对通知设置，这里不会再次发送。
      </p>
      <p v-else-if="state === 'confirmed'">此账号已有取消该歌手通知的确认记录。</p>
      <p v-else-if="state === 'ready'">后续将屏蔽此歌手的新歌通知</p>
      <p v-else-if="busy">正在检查通知操作记录…</p>
      <p v-if="error" role="alert">{{ error }}</p>
      <footer>
        <button
          ref="closeButton"
          type="button"
          class="download-button"
          :disabled="busy"
          @click="close"
        >
          {{ state === 'ready' ? '取消' : '关闭' }}
        </button>
        <button
          v-if="state === 'ready'"
          type="button"
          class="primary"
          :disabled="busy"
          @click="request('disable')"
        >
          {{ busy ? '正在处理…' : '确认' }}
        </button>
      </footer>
    </dialog>
  </div>
</template>
