<script setup lang="ts">
import { computed, nextTick, onMounted, onBeforeUnmount, ref, shallowRef, watch } from 'vue';
import type { PlaylistView } from '../../services/contracts';
import type { PlaylistManager } from '../../composables/usePlaylists';
import { playlistPending, type PlaylistMutationView } from '../../services/playlists';
import Icon from '../Icon.vue';
const props = defineProps<{
  playlist: PlaylistView;
  manager: PlaylistManager;
  subscribe?: boolean;
}>();
const emit = defineEmits<{ close: [] }>();
const dialog = ref<HTMLDialogElement>();
const checking = ref(true);
const readError = ref('');
const interrupted = shallowRef<PlaylistMutationView>();
const { busy, error, loading, editor, result } = props.manager;
const deleting = props.playlist.owned;
const kind = deleting ? 'delete' : 'subscribe';
const title = deleting ? '删除歌单' : props.subscribe ? '收藏歌单' : '取消收藏';
const receipt = computed(() => result.value ?? interrupted.value);
const pending = computed(() => !!receipt.value && playlistPending(receipt.value.phase));
const ready = computed(
  () =>
    !deleting ||
    (editor.value?.id === props.playlist.id && editor.value.owned && !editor.value.favorite),
);
let alive = true;
watch(pending, async (value) => {
  const focused = dialog.value?.querySelector('footer')?.contains(document.activeElement);
  if (!value || !focused) return;
  await nextTick();
  if (alive) dialog.value?.querySelector<HTMLElement>('[data-reconcile]')?.focus();
});
onBeforeUnmount(() => {
  alive = false;
});
onMounted(async () => {
  props.manager.reset();
  dialog.value?.showModal();
  // Preserve unresolved operations on reopening as well as within this dialog.
  await props.manager.refreshOperations();
  if (!alive) return;
  interrupted.value = props.manager.operations.value.find(
    (op) => op.playlistId === props.playlist.id && op.kind === kind && playlistPending(op.phase),
  );
  if (deleting && !pending.value) await read();
  if (alive) checking.value = false;
});
async function read() {
  if (loading.value || !alive) return;
  const retryFocused = document.activeElement?.matches('[data-playlist-read]');
  await props.manager.open(props.playlist.id);
  if (!alive) return;
  readError.value = error.value || (ready.value ? '' : '歌单已改变，无法从此入口删除。');
  if (retryFocused && ready.value && !readError.value) {
    await nextTick();
    dialog.value?.querySelector<HTMLElement>('[autofocus]')?.focus();
  }
}
async function submit() {
  if (!alive || busy.value || checking.value || loading.value || !ready.value || pending.value)
    return;
  const before = new Set(props.manager.operations.value.map((op) => op.operationId));
  const value = await props.manager.mutate(
    deleting
      ? { kind: 'delete', id: props.playlist.id, revision: editor.value!.revision }
      : { kind: 'subscribe', id: props.playlist.id, subscribed: !!props.subscribe },
  );
  if (!alive) return;
  if (value && props.manager.saved(value.phase)) emit('close');
  else if (!value)
    interrupted.value = props.manager.operations.value.find(
      (op) =>
        !before.has(op.operationId) &&
        op.playlistId === props.playlist.id &&
        op.kind === kind &&
        playlistPending(op.phase),
    );
}
async function reconcile() {
  if (!receipt.value || busy.value) return;
  const value = await props.manager.reconcile(receipt.value.operationId);
  if (alive && value && props.manager.saved(value.phase)) emit('close');
}
</script>
<template>
  <dialog
    ref="dialog"
    class="playlist-dialog playlist-mutation-dialog"
    aria-labelledby="playlist-mutation-title"
    :aria-busy="busy || checking || loading"
    @cancel.prevent="!busy && emit('close')"
  >
    <header>
      <h2 id="playlist-mutation-title">{{ title }}</h2>
      <button class="icon-button" aria-label="关闭歌单确认" :disabled="busy" @click="emit('close')">
        <Icon name="close" />
      </button>
    </header>
    <div class="playlist-mutation-body">
      <p v-if="deleting">确定删除「{{ playlist.name }}」？该歌单将从你的账号中移除。</p>
      <p v-else-if="subscribe">收藏「{{ playlist.name }}」到我的歌单？</p>
      <p v-else>取消收藏「{{ playlist.name }}」？之后可重新收藏。</p>
      <p v-if="checking || loading" class="muted" role="status">正在读取歌单…</p>
      <p v-if="pending" class="playlist-operation-message" role="status">
        操作结果还未确认。请先核对结果，避免重复操作；也可以关闭后在歌单操作记录中继续处理。
      </p>
      <p v-else-if="receipt" class="playlist-operation-message" role="status">
        {{ receipt.message }}
      </p>
      <div v-if="error || readError" class="playlist-operation-error" role="alert">
        {{ error || readError
        }}<button
          v-if="deleting && !ready && !pending"
          data-playlist-read
          class="text-button"
          :aria-disabled="loading"
          @click="read"
        >
          {{ loading ? '正在读取…' : '重新读取' }}
        </button>
      </div>
    </div>
    <footer>
      <button class="download-button" autofocus :disabled="busy" @click="emit('close')">
        {{ pending ? '稍后处理' : '取消' }}
      </button>
      <button
        v-if="pending"
        data-reconcile
        class="primary"
        :aria-disabled="busy"
        @click="reconcile"
      >
        {{ busy ? '正在核对…' : '核对结果' }}
      </button>
      <button
        v-else
        class="primary"
        :class="{ 'destructive-action': !subscribe }"
        :aria-disabled="busy || checking || loading || !ready"
        @click="submit"
      >
        {{ busy ? '正在处理…' : title }}
      </button>
    </footer>
  </dialog>
</template>
