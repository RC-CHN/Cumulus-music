<script setup lang="ts">
import { nextTick, onBeforeUnmount, onMounted, ref, useId, watch } from 'vue';

const props = defineProps<{ text: string }>();
const contentId = useId();
const body = ref<HTMLElement>();
const measure = ref<HTMLElement>();
const visible = ref<HTMLParagraphElement>();
const toggle = ref<HTMLButtonElement>();
const expanded = ref(false);
const preview = ref<string>();
const overflows = ref(false);
let frame: number | undefined;
let forceNext = false;
let observer: ResizeObserver | undefined;
let alive = false;
let measuredWidth = 0;
let measuredText = '';

function refreshPreview(force = false) {
  const node = measure.value;
  const width = node?.clientWidth ?? 0;
  if (!node || !width || (!force && width === measuredWidth && props.text === measuredText)) return;
  measuredWidth = width;
  measuredText = props.text;
  const height = parseFloat(getComputedStyle(node).lineHeight) * 4 + 1;
  node.textContent = props.text;
  overflows.value = node.scrollHeight > height;
  if (!overflows.value) {
    const restoreFocus = document.activeElement === toggle.value;
    preview.value = props.text;
    expanded.value = false;
    if (restoreFocus) {
      void nextTick(() => {
        if (document.activeElement === document.body) visible.value?.focus({ preventScroll: true });
      });
    }
  } else {
    // Put only the displayed characters in the selectable paragraph. A
    // visual clamp alone would also copy the text clipped by the viewport.
    const units = Array.from(props.text);
    const candidate = (length: number) => `${units.slice(0, length).join('').trimEnd()}…`;
    let fits = 0;
    let exceeds = units.length;
    while (exceeds - fits > 1) {
      const middle = (fits + exceeds) >>> 1;
      node.textContent = candidate(middle);
      if (node.scrollHeight > height) exceeds = middle;
      else fits = middle;
    }
    preview.value = candidate(fits);
  }
  node.textContent = '';
}

function schedule(force = false) {
  forceNext ||= force;
  if (frame !== undefined) return;
  frame = requestAnimationFrame(() => {
    frame = undefined;
    const requested = forceNext;
    forceNext = false;
    refreshPreview(requested);
  });
}
watch(
  () => props.text,
  () => {
    expanded.value = false;
    preview.value = undefined;
    overflows.value = false;
    schedule();
  },
  { flush: 'post' },
);
onMounted(() => {
  alive = true;
  observer = new ResizeObserver(() => schedule());
  if (body.value) observer.observe(body.value);
  schedule();
  void document.fonts?.ready.then(() => {
    if (alive) schedule(true);
  });
});
onBeforeUnmount(() => {
  alive = false;
  observer?.disconnect();
  if (frame !== undefined) cancelAnimationFrame(frame);
});
</script>

<template>
  <section class="voice-description-card" :aria-labelledby="`${contentId}-heading`">
    <h2 :id="`${contentId}-heading`" data-vinyl-copy-text>简介</h2>
    <div ref="body" class="voice-description-body">
      <p
        ref="visible"
        :id="contentId"
        class="voice-description-text"
        data-voice-description-text
        :data-vinyl-copy-text="preview === undefined ? undefined : ''"
        :class="{ 'voice-description-measuring': preview === undefined }"
        tabindex="-1"
      >
        {{ expanded ? text : (preview ?? text) }}
      </p>
      <p
        ref="measure"
        class="voice-description-text voice-description-measure"
        aria-hidden="true"
      />
    </div>
    <button
      v-if="overflows"
      ref="toggle"
      class="text-button voice-description-toggle"
      type="button"
      data-voice-description-toggle
      data-non-selectable
      :aria-controls="contentId"
      :aria-expanded="expanded"
      @click="expanded = !expanded"
    >
      {{ expanded ? '收起' : '展开' }}
    </button>
  </section>
</template>

<style scoped>
.voice-description-card {
  padding: 20px 24px;
  border-radius: 12px;
  background: color-mix(in srgb, var(--text) 7%, transparent);
}
.voice-description-card h2 {
  margin: 0 0 10px;
  font-size: 20px;
  line-height: 24px;
}
.voice-description-body {
  position: relative;
}
.voice-description-text {
  margin: 0;
  font-size: 16px;
  line-height: 26px;
  text-align: justify;
  white-space: break-spaces;
  overflow-wrap: anywhere;
}
.voice-description-measure {
  position: absolute;
  inset: 0 0 auto;
  visibility: hidden;
  pointer-events: none;
}
.voice-description-measuring {
  max-height: 104px;
  overflow: hidden;
}
.voice-description-toggle {
  display: block;
  margin-inline-start: auto;
  color: inherit;
  font-weight: 600;
}
</style>
