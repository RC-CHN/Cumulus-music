<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref } from 'vue';
import type { Page } from '../composables/useMusic';
import { choiceTabs } from '../services/choiceNavigation';
import type { VerticalZoneMenu, VerticalZoneMenuEntry } from '../services/verticalZone';
import Icon from './Icon.vue';
import MenuTransition from './MenuTransition.vue';
const props = defineProps<{ page: Page; zones?: VerticalZoneMenu }>();
const emit = defineEmits<{ navigate: [page: Page] }>();
const more = ref(false),
  wrapper = ref<HTMLElement>(),
  trigger = ref<HTMLButtonElement>(),
  panel = ref<HTMLElement>();
const primary = computed(() => (props.zones?.available ? props.zones.defaultZoneTags : []));
const extra = computed(() => (props.zones?.available ? props.zones.moreZoneTags : []));
const current = computed(() =>
  props.page.kind === 'vertical_zone' ? props.page.pageCode : undefined,
);
function close(restore = false) {
  more.value = false;
  if (restore) trigger.value?.focus({ preventScroll: true });
}
function open(zone: VerticalZoneMenuEntry) {
  close(zone.pageCode === current.value);
  if (zone.pageCode !== current.value)
    emit('navigate', { kind: 'vertical_zone', pageCode: zone.pageCode, choice: true });
}
function toggle() {
  if (more.value) close();
  else {
    more.value = true;
    void nextTick(() =>
      panel.value?.querySelector<HTMLButtonElement>('button[aria-current],button')?.focus(),
    );
  }
}
function outside(event: PointerEvent) {
  if (more.value && !wrapper.value?.contains(event.target as Node)) close();
}
onMounted(() => document.addEventListener('pointerdown', outside, true));
onBeforeUnmount(() => document.removeEventListener('pointerdown', outside, true));
</script>

<template>
  <nav ref="wrapper" class="choice-tabs" aria-label="精选分类" @keydown.esc="more && close(true)">
    <div class="choice-tabs-scroll">
      <button
        v-for="tab in choiceTabs"
        :key="tab.label"
        type="button"
        :aria-current="page.kind === tab.page.kind ? 'page' : undefined"
        @click="page.kind !== tab.page.kind && emit('navigate', tab.page)"
      >
        {{ tab.label }}
      </button>
      <button
        v-for="zone in primary"
        :key="zone.pageCode"
        type="button"
        :aria-current="current === zone.pageCode ? 'page' : undefined"
        :data-choice-zone="zone.pageCode"
        @click="open(zone)"
      >
        {{ zone.name }}
      </button>
    </div>
    <button
      v-if="extra.length"
      ref="trigger"
      type="button"
      class="choice-zone-more"
      :aria-current="extra.some((zone) => zone.pageCode === current) ? 'page' : undefined"
      :aria-expanded="more"
      @click="toggle"
    >
      更多 <Icon name="down" />
    </button>
    <MenuTransition>
      <div v-if="more && extra.length" ref="panel" class="choice-zone-panel" aria-label="更多专区">
        <button
          v-for="zone in extra"
          :key="zone.pageCode"
          type="button"
          :data-choice-zone="zone.pageCode"
          :aria-current="current === zone.pageCode ? 'page' : undefined"
          @click="open(zone)"
        >
          {{ zone.name }}
        </button>
      </div>
    </MenuTransition>
  </nav>
</template>

<style scoped>
.choice-tabs {
  display: flex;
  position: relative;
  flex: none;
  align-items: center;
  gap: 12px;
  padding: 0 clamp(24px, 3.3vw, 48px);
  min-height: 44px;
  color: var(--text-muted);
  border-bottom: 1px solid var(--line);
  z-index: 2;
}
.choice-tabs-scroll {
  display: flex;
  align-items: center;
  gap: clamp(12px, 2.5vw, 34px);
  min-width: 0;
  overflow: auto hidden;
  scrollbar-width: none;
}
button {
  flex: none;
  min-height: 44px;
  padding-inline: 4px;
  background: none;
  color: inherit;
  border: 0;
  border-bottom: 2px solid transparent;
  white-space: nowrap;
}
button[aria-current='page'] {
  color: var(--text);
  border-bottom-color: var(--accent);
  font-weight: 650;
}
.choice-zone-more {
  display: inline-flex;
  align-items: center;
  gap: 4px;
}
.choice-zone-more svg {
  width: 16px;
  transition: transform 200ms;
}
.choice-zone-more[aria-expanded='true'] svg {
  transform: rotate(180deg);
}
.choice-zone-panel {
  position: absolute;
  inset: calc(100% + 8px) clamp(12px, 3.3vw, 48px) auto auto;
  width: min(600px, calc(100% - 24px));
  max-height: min(360px, 58vh);
  overflow: auto;
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(80px, 1fr));
  gap: 16px 12px;
  padding: 24px 16px;
  border: 1px solid var(--line);
  border-radius: 12px;
  background: var(--surface-raised);
  box-shadow: 0 8px 24px #0002;
}
.choice-zone-panel button {
  min-height: 30px;
  overflow: hidden;
  text-overflow: ellipsis;
  padding-inline: 6px;
  border: 0;
  border-radius: 16px;
  background: var(--surface-alt);
}
.choice-zone-panel button[aria-current='page'] {
  color: var(--accent);
}
@media (prefers-reduced-motion: reduce) {
  .choice-zone-more svg {
    transition: none;
  }
}
</style>
