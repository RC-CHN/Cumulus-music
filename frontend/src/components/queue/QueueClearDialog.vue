<script setup lang="ts">
import { onMounted, ref } from 'vue';
import Icon from '../Icon.vue';
defineProps<{ count: number; busy: boolean; changed: boolean; blocked: boolean; issue: string }>();
const emit = defineEmits<{ close: []; confirm: [event: Event] }>();
const dialog = ref<HTMLDialogElement>(),
  cancel = ref<HTMLButtonElement>();
onMounted(() => {
  dialog.value?.showModal();
  cancel.value?.focus();
});
</script>
<template>
  <dialog
    ref="dialog"
    class="playlist-dialog queue-clear-dialog"
    aria-labelledby="queue-clear-title"
    aria-describedby="queue-clear-description"
    :aria-busy="busy"
    @keydown.esc.stop
    @cancel.prevent="!busy && emit('close')"
  >
    <header>
      <h2 id="queue-clear-title">确定要清空播放列表吗？</h2>
      <button
        class="icon-button"
        aria-label="关闭清空播放列表确认"
        :disabled="busy"
        @click="emit('close')"
      >
        <Icon name="close" />
      </button>
    </header>
    <p id="queue-clear-description">
      清空当前播放列表中的 {{ count }} 首歌曲，并停止播放。歌单和本地音乐文件会保留。
    </p>
    <p v-if="changed" class="form-warning" role="alert">
      播放列表已变化或暂时无法读取，请取消后核对当前列表。
    </p>
    <p v-else-if="blocked && !busy" class="muted" role="status">正在更新播放状态，请稍候。</p>
    <p v-if="issue" class="form-warning" role="alert">{{ issue }}</p>
    <footer>
      <button ref="cancel" class="secondary" :disabled="busy" @click="emit('close')">取消</button
      ><button
        class="primary"
        :disabled="busy || changed || blocked"
        @click="emit('confirm', $event)"
      >
        {{ busy ? '正在清空…' : '确认清空' }}
      </button>
    </footer>
  </dialog>
</template>
<style scoped>
.queue-clear-dialog {
  width: 420px;
  max-width: calc(100vw - 32px);
}
</style>
