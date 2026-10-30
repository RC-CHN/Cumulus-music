<script setup lang="ts">
import { nextTick, onMounted, onUnmounted, ref, watch } from 'vue';
const props = defineProps<{ resetKey: string }>();
const text = ref(''),
  left = ref(0),
  top = ref(0),
  positioned = ref(false),
  box = ref<HTMLElement>();
let anchor: HTMLElement | undefined,
  timer: ReturnType<typeof setTimeout> | undefined,
  serial = 0;
function hide() {
  serial++;
  if (timer) clearTimeout(timer);
  timer = undefined;
  anchor = undefined;
  text.value = '';
  positioned.value = false;
}
function enter(event: Event) {
  const target =
    event.target instanceof Element ? event.target.closest<HTMLElement>('[data-tooltip]') : null;
  if (target === anchor) return;
  hide();
  if (!target?.dataset.tooltip) return;
  anchor = target;
  const generation = serial;
  timer = setTimeout(
    async () => {
      if (generation !== serial || !target.isConnected) return;
      text.value = target.dataset.tooltip ?? '';
      await nextTick();
      if (generation !== serial || !box.value) return;
      const r = target.getBoundingClientRect(),
        tip = box.value.getBoundingClientRect();
      left.value = Math.max(12, Math.min(r.left, innerWidth - tip.width - 12));
      top.value =
        r.bottom + 8 + tip.height <= innerHeight - 12
          ? r.bottom + 8
          : Math.max(12, r.top - tip.height - 8);
      positioned.value = true;
    },
    event.type === 'focusin' ? 0 : 450,
  );
}
function leave(event: Event) {
  const next = (event as FocusEvent | PointerEvent).relatedTarget;
  if (anchor && next instanceof Node && anchor.contains(next)) return;
  hide();
}
const keys = (e: KeyboardEvent) => {
  if (e.key === 'Escape') hide();
};
onMounted(() => {
  document.addEventListener('pointerover', enter);
  document.addEventListener('pointerout', leave);
  document.addEventListener('focusin', enter);
  document.addEventListener('focusout', leave);
  document.addEventListener('scroll', hide, true);
  window.addEventListener('resize', hide);
  document.addEventListener('keydown', keys);
});
onUnmounted(() => {
  hide();
  document.removeEventListener('pointerover', enter);
  document.removeEventListener('pointerout', leave);
  document.removeEventListener('focusin', enter);
  document.removeEventListener('focusout', leave);
  document.removeEventListener('scroll', hide, true);
  window.removeEventListener('resize', hide);
  document.removeEventListener('keydown', keys);
});
watch(() => props.resetKey, hide);
</script>
<template>
  <Teleport to="body"
    ><div
      v-if="text"
      ref="box"
      role="tooltip"
      class="content-tooltip"
      :style="{ left: `${left}px`, top: `${top}px`, visibility: positioned ? 'visible' : 'hidden' }"
    >
      {{ text }}
    </div></Teleport
  >
</template>
<style scoped>
.content-tooltip {
  position: fixed;
  z-index: 90;
  pointer-events: none;
  max-width: min(320px, calc(100vw - 24px));
  padding: 7px 10px;
  border: 1px solid var(--line-strong);
  border-radius: 6px;
  background: var(--surface-raised);
  color: var(--text);
  font-size: 12px;
  line-height: 18px;
  overflow-wrap: anywhere;
  box-shadow: var(--shadow);
}
</style>
