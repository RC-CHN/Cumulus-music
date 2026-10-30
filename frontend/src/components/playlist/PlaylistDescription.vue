<script setup lang="ts">
import MenuTransition from '../MenuTransition.vue';
import { nextTick, onMounted, onUnmounted, ref, useId, watch } from 'vue';
import Icon from '../Icon.vue';

const props = defineProps<{ text: string }>();
const id = useId();
const paragraph = ref<HTMLElement>();
const trigger = ref<HTMLButtonElement>();
const panel = ref<HTMLElement>();
const closeButton = ref<HTMLButtonElement>();
const truncated = ref(false);
const open = ref(false);
let observer: ResizeObserver | undefined;
let mounted = false;

function measure() {
  const node = paragraph.value;
  if (mounted && node) truncated.value = node.scrollHeight > node.clientHeight + 1;
}
function close(restore = false) {
  open.value = false;
  if (restore) trigger.value?.focus({ preventScroll: true });
}
async function expand() {
  open.value = true;
  await nextTick();
  if (open.value) closeButton.value?.focus({ preventScroll: true });
}
function outside(event: Event) {
  if (
    open.value &&
    !panel.value?.contains(event.target as Node) &&
    !trigger.value?.contains(event.target as Node)
  )
    close();
}
function readingKey(event: KeyboardEvent) {
  // Keep Space/Shift+Space available for native scrolling inside the reader;
  // the application-level playback shortcut must not consume this event.
  if (event.code === 'Space' && !event.ctrlKey && !event.altKey && !event.metaKey)
    event.stopPropagation();
}
onMounted(() => {
  mounted = true;
  observer = new ResizeObserver(measure);
  if (paragraph.value) observer.observe(paragraph.value);
  measure();
  void document.fonts.ready.then(measure);
  document.addEventListener('pointerdown', outside, true);
  document.addEventListener('focusin', outside, true);
});
onUnmounted(() => {
  mounted = false;
  observer?.disconnect();
  document.removeEventListener('pointerdown', outside, true);
  document.removeEventListener('focusin', outside, true);
});
watch(
  () => props.text,
  async () => {
    close();
    await nextTick();
    observer?.disconnect();
    if (paragraph.value) observer?.observe(paragraph.value);
    measure();
  },
);
</script>

<template>
  <div v-if="text" class="playlist-description">
    <div class="playlist-description-preview" :class="{ truncated }">
      <p ref="paragraph" :title="text">{{ text }}</p>
      <button
        v-if="truncated"
        ref="trigger"
        class="playlist-description-expand"
        aria-label="展开歌单简介"
        :aria-expanded="open"
        :aria-controls="open ? id : undefined"
        aria-haspopup="dialog"
        @click="open ? close(true) : expand()"
      >
        详情
      </button>
    </div>
    <MenuTransition>
      <div
        v-if="open"
        :id="id"
        ref="panel"
        class="playlist-description-overlay"
        role="dialog"
        aria-label="歌单简介"
        @keydown.esc.stop.prevent="close(true)"
      >
        <div
          class="playlist-description-scroll"
          tabindex="0"
          aria-label="完整歌单简介"
          @keydown="readingKey"
        >
          {{ text }}
        </div>
        <button
          ref="closeButton"
          class="icon-button"
          aria-label="收起歌单简介"
          @click="close(true)"
        >
          <Icon name="close" />
        </button>
      </div>
    </MenuTransition>
  </div>
</template>
