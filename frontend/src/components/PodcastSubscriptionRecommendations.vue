<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, shallowRef, watch } from 'vue';
import {
  podcastPublicLink,
  type PodcastRecommendationAction,
  type PodcastSubscriptionRecommendation,
} from '../services/podcast';
import type { MenuItem } from '../services/menu';
import { useCore } from '../services/core';
import ActionMenu from './ActionMenu.vue';
import Icon from './Icon.vue';
import ModalTransition from './ModalTransition.vue';
import TrackShareDialog from './TrackShareDialog.vue';

const props = defineProps<{
  cards: PodcastSubscriptionRecommendation[];
  revision: string;
  canCollect: (id: string) => boolean;
  disabled: boolean;
  canPlay: boolean;
  currentRadioId?: string;
  playing: boolean;
}>();
const emit = defineEmits<{
  open: [id: string];
  play: [id: string, action: PodcastRecommendationAction];
  collect: [id: string, revision: string];
}>();
const core = useCore();
const viewport = ref<HTMLElement>();
const menu = ref<InstanceType<typeof ActionMenu>>();
type CardTarget = {
  card: PodcastSubscriptionRecommendation;
  revision: string;
  anchor: HTMLButtonElement;
};
const target = shallowRef<CardTarget>();
function isCurrent(value: CardTarget) {
  return (
    value.revision === props.revision &&
    props.cards.includes(value.card) &&
    !value.card.unavailable &&
    value.anchor.isConnected &&
    !value.anchor.closest('[inert]')
  );
}
const selected = computed(() =>
  target.value && isCurrent(target.value) ? target.value.card : undefined,
);
const menuItems = computed<MenuItem[]>(() => {
  const canShare = !!selected.value && !!podcastPublicLink(selected.value.id);
  return [
    { key: 'open', label: '查看播客', icon: 'search' },
    { key: 'play', label: '播放', icon: 'play', disabled: !props.canPlay },
    { key: 'next', label: '下一首播放', icon: 'next', disabled: !props.canPlay },
    { key: 'sep', label: '', separator: true },
    ...(selected.value && props.canCollect(selected.value.id)
      ? [{ key: 'collect', label: '收藏...', icon: 'favorite' }]
      : []),
    { key: 'share', label: '分享...', icon: 'share', disabled: !canShare },
    { key: 'copy', label: '复制链接', icon: 'copy', disabled: !canShare },
  ];
});
const shared = shallowRef<CardTarget>();
const collect = shallowRef<CardTarget>();
const collectDialog = ref<HTMLDialogElement>();
watch(collectDialog, (dialog) => dialog?.showModal());
watch([() => props.revision, () => props.cards], () => {
  collect.value = undefined;
  shared.value = undefined;
  menu.value?.close();
});
const feedback = ref('');
let timer: ReturnType<typeof setTimeout> | undefined;
const width = ref(0),
  start = ref(0);
const gap = 20;
// GridCarousel uses miniNum=4 (span 3), capped at six cards.
const columns = computed(() => Math.max(1, Math.min(6, Math.floor((width.value + gap) / 198))));
const maxStart = computed(() => Math.max(0, props.cards.length - columns.value));
const cell = computed(() => Math.max(0, (width.value - gap * (columns.value - 1)) / columns.value));
const count = new Intl.NumberFormat('zh-CN', { notation: 'compact', maximumFractionDigits: 1 });
let resize: ResizeObserver | undefined;
onMounted(() => {
  if (!viewport.value) return;
  width.value = viewport.value.clientWidth;
  resize = new ResizeObserver(([entry]) => {
    width.value = entry.contentRect.width;
  });
  resize.observe(viewport.value);
});
onBeforeUnmount(() => {
  resize?.disconnect();
  clearTimeout(timer);
  target.value = undefined;
});
watch(columns, () => (start.value = 0));
watch(maxStart, (limit) => (start.value = Math.min(start.value, limit)));
function move(direction: -1 | 1) {
  menu.value?.close();
  start.value = Math.max(0, Math.min(maxStart.value, start.value + direction * columns.value));
}
async function context(event: MouseEvent | KeyboardEvent, card: PodcastSubscriptionRecommendation) {
  if (props.disabled || card.unavailable || !(event.currentTarget instanceof HTMLElement)) return;
  const node = event.currentTarget;
  if (node.closest('[inert]')) return;
  const anchor =
    (event.target instanceof Element ? event.target.closest<HTMLButtonElement>('button') : null) ??
    node.querySelector<HTMLButtonElement>('.recommendation-open');
  if (!anchor?.isConnected) return;
  const box = anchor.getBoundingClientRect();
  const pointer = event instanceof MouseEvent && event.button === 2;
  const point = { x: pointer ? event.clientX : box.left, y: pointer ? event.clientY : box.bottom };
  const selection = { card, revision: props.revision, anchor };
  target.value = selection;
  await nextTick();
  if (target.value === selection && anchor.isConnected && selected.value)
    void menu.value?.showAt(point.x, point.y, anchor);
}
function keyboard(event: KeyboardEvent, card: PodcastSubscriptionRecommendation) {
  if (event.key !== 'ContextMenu' && !(event.shiftKey && event.key === 'F10')) return;
  event.preventDefault();
  event.stopPropagation();
  void context(event, card);
}
async function menuAction(key: string) {
  const card = selected.value,
    selection = target.value;
  if (
    props.disabled ||
    !card ||
    !selection?.anchor.isConnected ||
    selection.anchor.closest('[inert]')
  )
    return;
  if (key === 'open') emit('open', card.id);
  if ((key === 'play' || key === 'next') && props.canPlay) emit('play', card.id, key);
  if (key === 'share') shared.value = selection;
  if (key === 'collect' && props.canCollect(card.id)) collect.value = selection;
  if (key === 'copy') {
    const link = podcastPublicLink(card.id);
    if (!link) return;
    let message: string;
    try {
      await core.copyText(link);
      message = '链接已复制';
    } catch {
      message = '复制失败，请重试';
    }
    if (target.value !== selection) return;
    clearTimeout(timer);
    feedback.value = message;
    timer = setTimeout(() => {
      feedback.value = '';
    }, 3500);
  }
}
function confirmCollect() {
  const selection = collect.value;
  collect.value = undefined;
  if (selection && isCurrent(selection) && props.canCollect(selection.card.id))
    emit('collect', selection.card.id, selection.revision);
}
</script>

<template>
  <section
    class="subscription-recommendations"
    aria-label="为你推荐"
    data-podcast-subscription-recommendations
  >
    <h2>为你推荐</h2>
    <div ref="viewport" class="recommendation-viewport">
      <div
        class="recommendation-rail"
        :style="{
          gridAutoColumns: cell + 'px',
          transform: 'translateX(' + -(cell + gap) * start + 'px)',
        }"
      >
        <article
          v-for="(card, index) in cards"
          :key="card.id"
          class="recommendation-card"
          :data-recommended-radio-id="card.id"
          :inert="index < start || index >= start + columns"
          @contextmenu.prevent.stop="context($event, card)"
          @keydown="keyboard($event, card)"
        >
          <button
            type="button"
            class="recommendation-open"
            :disabled="disabled || card.unavailable"
            :aria-haspopup="card.unavailable ? undefined : 'menu'"
            aria-expanded="false"
            @click="emit('open', card.id)"
          >
            <span class="recommendation-cover">
              <img
                v-if="card.coverUrl"
                :src="card.coverUrl"
                alt=""
                loading="lazy"
                referrerpolicy="no-referrer"
              />
              <Icon v-else name="music" />
              <span
                v-if="card.playCount !== null && card.playCount > 0"
                class="recommendation-count"
              >
                <Icon name="headphones" />{{ count.format(card.playCount) }}
              </span>
            </span>
            <span class="recommendation-name" :title="card.name">{{ card.name }}</span>
          </button>
          <button
            v-if="!card.unavailable"
            type="button"
            class="recommendation-play"
            :class="{ active: currentRadioId === card.id }"
            :disabled="disabled || !canPlay"
            :aria-label="
              (currentRadioId === card.id ? (playing ? '暂停 ' : '继续播放 ') : '播放 ') + card.name
            "
            @click="emit('play', card.id, 'overlay')"
          >
            <Icon :name="currentRadioId === card.id && playing ? 'pause' : 'play'" />
          </button>
        </article>
      </div>
    </div>
    <template v-if="maxStart > 0">
      <button
        class="recommendation-arrow recommendation-previous"
        type="button"
        aria-label="上一组推荐播客"
        :aria-disabled="start === 0"
        @click="move(-1)"
      >
        <Icon name="back" />
      </button>
      <button
        class="recommendation-arrow recommendation-next"
        type="button"
        aria-label="下一组推荐播客"
        :aria-disabled="start === maxStart"
        @click="move(1)"
      >
        <Icon name="back" />
      </button>
    </template>
    <ActionMenu
      ref="menu"
      context-only
      variant="song"
      :label="`推荐播客菜单：${selected?.name ?? ''}`"
      :items="menuItems"
      :disabled="disabled || !selected"
      @select="menuAction"
    />
    <ModalTransition :return-focus="() => shared?.anchor ?? null">
      <TrackShareDialog v-if="shared" :podcast="shared.card" @close="shared = undefined" />
    </ModalTransition>
    <ModalTransition :return-focus="() => collect?.anchor ?? null">
      <dialog
        v-if="collect"
        ref="collectDialog"
        class="playlist-dialog"
        aria-label="收藏播客"
        @cancel.prevent="collect = undefined"
      >
        <header>
          <h2>收藏播客</h2>
          <button class="icon-button" aria-label="关闭收藏确认" @click="collect = undefined">
            <Icon name="close" />
          </button>
        </header>
        <p>将「{{ collect.card.name }}」加入我的收藏？</p>
        <footer>
          <button class="secondary" autofocus @click="collect = undefined">取消</button>
          <button class="primary" @click="confirmCollect">确认收藏</button>
        </footer>
      </dialog>
    </ModalTransition>
    <p v-if="feedback" class="track-action-feedback" role="status">{{ feedback }}</p>
  </section>
</template>

<style scoped>
.subscription-recommendations {
  position: relative;
  padding-top: 39px;
  margin-bottom: 20px;
  min-width: 0;
}
.subscription-recommendations h2 {
  font-size: 22px;
  line-height: 1.4;
  margin: 0 0 13px;
}
.recommendation-viewport {
  overflow: hidden;
}
.recommendation-rail {
  display: grid;
  grid-auto-flow: column;
  gap: 20px;
  transition: transform 0.6s ease;
}
.recommendation-card {
  min-width: 0;
  position: relative;
}
.recommendation-open {
  display: block;
  width: 100%;
  border: 0;
  border-radius: 6px;
  overflow: hidden;
  padding: 0;
  background: var(--surface-alt);
  color: inherit;
  text-align: left;
}
.recommendation-open:focus-visible {
  outline-offset: -2px;
}
.recommendation-play {
  position: absolute;
  right: 10px;
  bottom: 54px;
  width: 38px;
  height: 38px;
  border: 0;
  border-radius: 50%;
  display: grid;
  place-items: center;
  background: var(--accent-fill);
  color: var(--on-accent);
  opacity: 0;
  pointer-events: none;
  transition: opacity 0.2s ease-in;
}
.recommendation-card:hover .recommendation-play,
.recommendation-card:focus-within .recommendation-play,
.recommendation-play.active {
  opacity: 1;
  pointer-events: auto;
}
.recommendation-play .icon {
  width: 21px;
  height: 21px;
}
.recommendation-cover {
  position: relative;
  display: grid;
  width: 100%;
  aspect-ratio: 1;
  place-items: center;
  background: #713948;
  color: white;
}
.recommendation-cover > img {
  width: 100%;
  height: 100%;
  object-fit: cover;
}
.recommendation-cover > .icon {
  width: 35%;
  height: 35%;
}
.recommendation-count {
  position: absolute;
  top: 0;
  inset-inline: 0;
  display: flex;
  gap: 4px;
  align-items: center;
  justify-content: flex-end;
  padding: 8px 10px;
  font-size: 12px;
  background: linear-gradient(#0007, transparent);
}
.recommendation-count .icon {
  width: 12px;
  height: 12px;
}
.recommendation-name {
  display: -webkit-box;
  -webkit-box-orient: vertical;
  -webkit-line-clamp: 2;
  overflow: hidden;
  min-height: 48px;
  padding: 6px 12px;
  overflow-wrap: anywhere;
}
.recommendation-arrow {
  position: absolute;
  top: calc(50% + 35px);
  width: 26px;
  height: 32px;
  border: 0;
  border-radius: 6px;
  color: inherit;
  background: var(--sidebar-bg);
  transform: translateY(-50%);
}
.recommendation-arrow[aria-disabled='true'] {
  opacity: 0.45;
  cursor: default;
}
.recommendation-previous {
  left: 0;
}
.recommendation-next {
  right: 0;
}
.recommendation-next .icon {
  transform: rotate(180deg);
}
@media (prefers-reduced-motion: reduce) {
  .recommendation-rail,
  .recommendation-play {
    transition: none;
  }
}
</style>
