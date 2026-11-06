<script setup lang="ts">
import { onMounted, ref, useId } from 'vue';

const emit = defineEmits<{ close: []; confirm: [] }>();
const id = useId();
const dialog = ref<HTMLDialogElement>();
const cancel = ref<HTMLButtonElement>();
onMounted(() => {
  dialog.value?.showModal();
  cancel.value?.focus({ preventScroll: true });
});
</script>

<template>
  <dialog
    ref="dialog"
    class="playlist-dialog voice-podcast-uncollect"
    data-voice-podcast-uncollect
    :aria-labelledby="id"
    @keydown.esc.stop
    @cancel.prevent="emit('close')"
  >
    <h2 :id="id">确定不再收藏该播单？</h2>
    <div class="track-remove-actions">
      <button ref="cancel" type="button" class="secondary" @click="emit('close')">取消</button>
      <button type="button" class="primary" @click="emit('confirm')">不再收藏</button>
    </div>
  </dialog>
</template>

<style scoped>
.voice-podcast-uncollect h2 {
  margin: 0 0 20px;
  font-size: 17px;
}
</style>
