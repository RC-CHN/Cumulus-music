<script setup lang="ts">
import { nextTick, ref, useId, watch } from 'vue';
import Icon from '../Icon.vue';
import PlaylistTagPopover from './PlaylistTagPopover.vue';
const props = defineProps<{
  modelValue: string[];
  tags: { name: string; category: string }[];
  loading: boolean;
  error: string;
  disabled: boolean;
}>();
const emit = defineEmits<{
  'update:modelValue': [value: string[]];
  retry: [];
  visibility: [open: boolean];
}>();
const open = ref(false),
  host = ref<HTMLElement>(),
  trigger = ref<HTMLButtonElement>();
const id = useId();
let original: string[] = [];
function toggle(tag: string) {
  if (props.disabled) return;
  emit(
    'update:modelValue',
    props.modelValue.includes(tag)
      ? props.modelValue.filter((t) => t !== tag)
      : [...props.modelValue, tag].slice(0, 3),
  );
}
async function close(restore = true, direction: -1 | 0 | 1 = 0) {
  open.value = false;
  emit('visibility', false);
  if (!restore) return;
  await nextTick();
  if (!trigger.value?.isConnected) return;
  let target: HTMLElement = trigger.value;
  if (direction) {
    const nodes = [
      ...document.querySelectorAll<HTMLElement>(
        'button:not(:disabled),input:not(:disabled),textarea:not(:disabled),[tabindex="0"]',
      ),
    ].filter(
      (node) =>
        !node.closest('.tag-picker') &&
        node.getClientRects().length &&
        getComputedStyle(node).visibility !== 'hidden',
    );
    target = nodes[nodes.indexOf(trigger.value) + direction] ?? trigger.value;
  }
  target.focus({ preventScroll: true });
}
function show() {
  if (props.disabled) return;
  if (open.value) void close();
  else {
    original = [...props.modelValue];
    emit('visibility', true);
    open.value = true;
  }
}
function cancel() {
  emit('update:modelValue', original);
  void close();
}
watch(
  () => props.disabled,
  (disabled) => {
    if (disabled) void close(false);
  },
);
</script>
<template>
  <div class="playlist-editor-tags" @keydown.esc.stop.prevent="open && close()">
    <div ref="host" class="tag-selection" role="group" aria-label="已选歌单标签">
      <button
        v-for="tag in modelValue"
        :key="tag"
        class="selected-tag"
        type="button"
        :disabled="disabled"
        :aria-label="`移除标签 ${tag}`"
        :title="tag"
        @click="toggle(tag)"
      >
        <span>{{ tag }}</span
        ><Icon name="close" />
      </button>
      <button
        ref="trigger"
        type="button"
        class="tag-select-trigger"
        aria-label="选择标签"
        aria-haspopup="dialog"
        :disabled="disabled"
        :aria-expanded="open"
        :aria-controls="open ? id : undefined"
        @click="show"
        @keydown.down.prevent="!open && show()"
      >
        <span v-if="!modelValue.length">选择标签</span><Icon name="down" />
      </button>
    </div>
    <Teleport to="body"
      ><Transition name="menu"
        ><PlaylistTagPopover
          v-if="open"
          :id="id"
          :anchor="host"
          :selected="modelValue"
          :tags="tags"
          :loading="loading"
          :error="error"
          :disabled="disabled"
          @toggle="toggle"
          @cancel="cancel"
          @confirm="close()"
          @dismiss="close"
          @retry="emit('retry')" /></Transition
    ></Teleport>
  </div>
</template>
