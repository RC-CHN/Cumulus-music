<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, reactive, ref, watch } from 'vue';
import type { VerticalZoneModel, ZoneViewport } from '../composables/useVerticalZone';
import { useMotion } from '../composables/useMotion';
import type { VerticalZoneBlock, VerticalZoneCard } from '../services/verticalZone';
import Icon from './Icon.vue';
import '../styles/vertical-zone.css';

const props = defineProps<{
  name: string;
  model: VerticalZoneModel;
  viewport: ZoneViewport;
  playable: boolean;
  selectedTab?: string;
  openingCard?: string;
  actionIssue?: string;
}>();
const emit = defineEmits<{
  tab: [key: string];
  open: [card: VerticalZoneCard];
  play: [card: VerticalZoneCard];
  privacy: [];
}>();
const root = ref<HTMLElement>();
const { data, error, loading, fmArtwork } = props.model;
const motion = useMotion();
const tabs = computed(() => props.model.tabs.value.filter((tab) => tab.name.trim()));
const active = computed(() => data.value?.activeTab?.key || props.selectedTab || 'default');
const retry = ref<HTMLButtonElement>();
let pendingRestore = true;
let observer: ResizeObserver | undefined;
const railBounds = reactive(new Map<string, { previous: boolean; next: boolean }>());
function measureRail(rail: HTMLElement) {
  const key = rail.dataset.zoneRail;
  if (!key) return;
  const previous = rail.scrollLeft > 1;
  const next = rail.scrollWidth - rail.clientWidth - rail.scrollLeft > 1;
  if (railBounds.get(key)?.previous !== previous || railBounds.get(key)?.next !== next)
    railBounds.set(key, { previous, next });
}
function measureRails() {
  root.value?.querySelectorAll<HTMLElement>('[data-zone-rail]').forEach(measureRail);
}
function focusKey(event: FocusEvent) {
  const target = event.target as HTMLElement;
  if (target.dataset.zoneFocus) props.viewport.focus = target.dataset.zoneFocus;
}
function scrollRail(block: number, direction: number) {
  const rail = root.value?.querySelector<HTMLElement>(`[data-zone-rail="${block}"]`);
  rail?.scrollBy({
    left: direction * rail.clientWidth,
    behavior: motion.reduced.value ? 'instant' : 'smooth',
  });
}
function railScroll(event: Event, index: number) {
  const rail = event.currentTarget as HTMLElement;
  if (!rail.dataset.zoneRail) return;
  props.viewport.rails.set(String(index), rail.scrollLeft);
  measureRail(rail);
}
function isCoverPlayback(block: VerticalZoneBlock, card: VerticalZoneCard) {
  return block.kind === 'carousel' && ['song', 'voice'].includes(card.target.kind);
}
function artwork(card: VerticalZoneCard) {
  return (
    (card.target.kind === 'fm' ? fmArtwork.value.get(card.target.id) : undefined) ?? card.cover
  );
}
function title(card: VerticalZoneCard) {
  return (
    card.title ??
    {
      album: '专辑',
      artist: '歌手',
      fm: '专区漫游',
      playlist: '歌单',
      podcast: '播客',
      song: '歌曲',
      user: '用户',
      voice: '声音',
    }[card.target.kind]
  );
}
async function restore() {
  await nextTick();
  const section = root.value;
  if (!section || !pendingRestore || loading.value || !data.value) return;
  pendingRestore = false;
  section.scrollTop = props.viewport.top;
  section.querySelectorAll<HTMLElement>('[data-zone-rail]').forEach((rail) => {
    rail.scrollLeft = props.viewport.rails.get(rail.dataset.zoneRail ?? '') ?? 0;
  });
  measureRails();
  const focused = document.activeElement;
  if (focused === document.body || focused?.getAttribute('aria-label') === '返回上一页') {
    const wanted = Array.from(section.querySelectorAll<HTMLElement>('[data-zone-focus]')).find(
      (button) => button.dataset.zoneFocus === props.viewport.focus,
    );
    wanted?.focus({ preventScroll: true });
  }
}
function retryPage() {
  root.value?.focus({ preventScroll: true });
  void props.model.read().then(async () => {
    await nextTick();
    if (error.value && document.activeElement === root.value) retry.value?.focus();
  });
}
watch(data, () => void restore(), { flush: 'post' });
onMounted(() => {
  if (root.value && typeof ResizeObserver !== 'undefined') {
    observer = new ResizeObserver(measureRails);
    observer.observe(root.value);
  }
  void restore();
});
onBeforeUnmount(() => observer?.disconnect());
defineExpose({ restore });
</script>

<template>
  <section
    ref="root"
    class="vertical-zone"
    :class="`zone-cards-${data?.cardSize ?? 'default'}`"
    :aria-label="name"
    :aria-busy="loading"
    tabindex="-1"
    @focusin="focusKey"
    @scroll.passive="viewport.top = ($event.currentTarget as HTMLElement).scrollTop"
  >
    <nav v-if="tabs.length >= 2" class="vertical-zone-subtabs" aria-label="专区分类">
      <button
        v-for="tab in tabs"
        :key="tab.key"
        type="button"
        :aria-current="tab.key === active ? 'page' : undefined"
        :data-zone-focus="`tab:${tab.key}`"
        @click="tab.key !== active && emit('tab', tab.key)"
      >
        {{ tab.name }}
      </button>
    </nav>
    <div v-if="loading && !data" class="empty-state" role="status">
      <span class="spinner" /> 正在加载专区…
    </div>
    <div v-if="error" class="empty-state" role="status">
      <p>{{ error }}</p>
      <button ref="retry" class="text-button" type="button" :disabled="loading" @click="retryPage">
        重试
      </button>
    </div>
    <p v-if="actionIssue" class="vertical-zone-action-issue" role="alert">{{ actionIssue }}</p>
    <div v-if="data && !data.blocks.length" class="empty-state">暂无专区内容</div>
    <section
      v-for="(block, index) in data?.blocks ?? []"
      :key="`${index}:${block.kind}`"
      class="vertical-zone-module"
      :class="`vertical-zone-${block.kind}`"
      :data-zone-module="block.kind"
    >
      <div
        v-if="
          block.title ||
          (!block.requiresRecommendation && ['horizontal', 'carousel'].includes(block.kind))
        "
        class="vertical-zone-heading"
      >
        <h2 v-if="block.title">{{ block.title }}</h2>
        <div
          v-if="!block.requiresRecommendation && ['horizontal', 'carousel'].includes(block.kind)"
          class="vertical-zone-arrows"
        >
          <button
            class="icon-button"
            type="button"
            aria-label="上一组"
            :disabled="!railBounds.get(String(index))?.previous"
            :data-zone-focus="`${index}:previous`"
            @click="scrollRail(index, -1)"
          >
            <Icon name="back" />
          </button>
          <button
            class="icon-button vertical-zone-next"
            type="button"
            aria-label="下一组"
            :disabled="!railBounds.get(String(index))?.next"
            :data-zone-focus="`${index}:next`"
            @click="scrollRail(index, 1)"
          >
            <Icon name="back" />
          </button>
        </div>
      </div>
      <div v-if="block.requiresRecommendation" class="vertical-zone-privacy">
        <p>个性化推荐被关闭了噢！</p>
        <button
          type="button"
          class="outline-button"
          :data-zone-focus="`${index}:privacy`"
          @click="emit('privacy')"
        >
          去设置页重新开启
        </button>
      </div>
      <div
        v-else
        class="vertical-zone-items"
        :data-zone-rail="['horizontal', 'carousel'].includes(block.kind) ? index : undefined"
        @scroll.passive="railScroll($event, index)"
      >
        <article
          v-for="(card, row) in block.cards"
          :key="`${card.target.kind}:${card.target.id}:${row}`"
          class="vertical-zone-card"
        >
          <div class="vertical-zone-picture">
            <button
              type="button"
              class="vertical-zone-cover"
              :aria-label="isCoverPlayback(block, card) ? `播放${title(card)}` : title(card)"
              :disabled="isCoverPlayback(block, card) && !playable"
              :aria-busy="openingCard === card.cardKey"
              :data-zone-focus="`${index}:${row}:open`"
              @click="isCoverPlayback(block, card) ? emit('play', card) : emit('open', card)"
            >
              <img v-if="artwork(card)" :src="artwork(card) ?? undefined" alt="" loading="lazy" />
              <Icon v-else :name="card.target.kind === 'fm' ? 'headphones' : 'music'" />
              <span v-if="card.target.kind === 'fm'" class="vertical-zone-fm-mask"
                >{{ name }}漫游</span
              >
            </button>
            <button
              v-if="card.canPlay && !isCoverPlayback(block, card)"
              class="vertical-zone-play"
              type="button"
              :disabled="!playable"
              :aria-label="`播放${title(card)}`"
              :data-zone-focus="`${index}:${row}:play`"
              @click="emit('play', card)"
            >
              <Icon name="play" />
            </button>
          </div>
          <div v-if="block.kind !== 'carousel'" class="vertical-zone-card-copy">
            <button
              class="vertical-zone-card-title"
              type="button"
              :aria-busy="openingCard === card.cardKey"
              :data-zone-focus="`${index}:${row}:title`"
              @click="emit('open', card)"
            >
              {{ title(card) }}
            </button>
            <ol v-if="card.previews.length" class="vertical-zone-previews">
              <li v-for="(preview, n) in card.previews" :key="n">
                <span>{{ preview.title }}</span
                ><small v-if="preview.artist">{{ preview.artist }}</small>
              </li>
            </ol>
          </div>
        </article>
      </div>
    </section>
  </section>
</template>
