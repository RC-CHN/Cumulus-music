<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, reactive, ref, watch } from 'vue';
import type { DiscoveryHomeModel } from '../composables/useDiscoveryHome';
import type { DiscoveryHomeCard, DiscoveryHomeSection } from '../services/discoveryHome';
import { useMotion } from '../composables/useMotion';
import Icon from './Icon.vue';
import RankingCards from './RankingCards.vue';
import type { RankingCard } from '../services/rankings';
import type { PlaybackStatus } from '../services/player';
import '../styles/discovery-home.css';

const props = defineProps<{
  model: DiscoveryHomeModel;
  playable: boolean;
  currentPlaylistId?: string;
  playbackState?: PlaybackStatus;
}>();
const emit = defineEmits<{
  open: [card: DiscoveryHomeCard, section: DiscoveryHomeSection];
  playPlaylist: [id: string];
  privacy: [];
  rankings: [];
  playlistSquare: [];
  openRank: [card: RankingCard];
}>();
const root = ref<HTMLElement>();
const { data, ranking, loading, error, viewport } = props.model;
const blocks = computed(() => {
  const entries = [...(data.value?.sections ?? [])] as Array<DiscoveryHomeSection | 'rankings'>;
  if (ranking.value) {
    const position = entries.findIndex(
      (entry) => entry !== 'rankings' && entry.kind !== 'playlists' && entry.kind !== 'featured',
    );
    entries.splice(position < 0 ? entries.length : position, 0, 'rankings');
  }
  return entries;
});
const motion = useMotion();
const failed = ref(new Set<string>());
const day = new Date().getDate();
const railBounds = reactive(new Map<string, { previous: boolean; next: boolean }>());
let observer: ResizeObserver | undefined;
function measureRail(rail: HTMLElement) {
  const kind = rail.dataset.homeRail;
  if (!kind) return;
  const previous = rail.scrollLeft > 1;
  const next = rail.scrollWidth - rail.clientWidth - rail.scrollLeft > 1;
  const existing = railBounds.get(kind);
  if (existing?.previous !== previous || existing?.next !== next)
    railBounds.set(kind, { previous, next });
}
function measureRails() {
  root.value?.querySelectorAll<HTMLElement>('[data-home-rail]').forEach(measureRail);
}
function rememberRankingFocus(event: FocusEvent) {
  const focused = event.target as HTMLElement;
  if (focused.dataset.homeCard) viewport.focus = focused.dataset.homeCard;
}
function railScrolled(event: Event) {
  const rail = event.currentTarget as HTMLElement;
  if (rail.dataset.homeRail) viewport.rails.set(rail.dataset.homeRail, rail.scrollLeft);
  measureRail(rail);
}
function key(section: DiscoveryHomeSection, card: DiscoveryHomeCard) {
  const { target } = card;
  const id = target.kind === 'style' ? target.seed.id : 'id' in target ? target.id : '';
  return section.kind + ':' + target.kind + ':' + id;
}
function scrollRail(event: MouseEvent, section: DiscoveryHomeSection, direction: number) {
  if (!railBounds.get(section.kind)?.[direction < 0 ? 'previous' : 'next']) return;
  const rail = root.value?.querySelector<HTMLElement>(`[data-home-rail="${section.kind}"]`);
  if (!rail) return;
  rail.scrollBy({
    left: direction * rail.clientWidth,
    behavior: motion.reduced.value ? 'instant' : 'smooth',
  });
  if (event.detail === 0) (event.currentTarget as HTMLButtonElement).focus({ preventScroll: true });
}
async function restore() {
  await nextTick();
  if (!root.value) return;
  root.value.scrollTop = viewport.scroll;
  for (const rail of root.value.querySelectorAll<HTMLElement>('[data-home-rail]'))
    rail.scrollLeft = viewport.rails.get(rail.dataset.homeRail ?? '') ?? 0;
  measureRails();
  const active = document.activeElement;
  if (
    viewport.focus &&
    (active === document.body || active?.getAttribute('aria-label') === '返回上一页')
  ) {
    for (const button of root.value.querySelectorAll<HTMLButtonElement>('[data-home-card]')) {
      if (button.dataset.homeCard === viewport.focus) {
        button.focus({ preventScroll: true });
        break;
      }
    }
  }
}
defineExpose({ restore });
watch(data, measureRails, { flush: 'post' });
onMounted(() => {
  if (root.value && typeof ResizeObserver !== 'undefined') {
    observer = new ResizeObserver(measureRails);
    observer.observe(root.value);
  }
  void restore();
});
onBeforeUnmount(() => observer?.disconnect());
</script>

<template>
  <section
    ref="root"
    class="discovery-home"
    :aria-busy="loading"
    @scroll.passive="viewport.scroll = ($event.currentTarget as HTMLElement).scrollTop"
  >
    <header class="page-heading discovery-home-heading">
      <h1>首页</h1>
      <button
        v-if="data?.recommendationEnabled"
        class="icon-button"
        type="button"
        aria-label="刷新首页"
        :disabled="loading"
        @click="model.refresh"
      >
        <Icon name="refresh" />
      </button>
    </header>
    <div v-if="data?.recommendationEnabled === false" class="empty-state">
      <p>个性化推荐被关闭了噢！</p>
      <button class="text-button" type="button" @click="emit('privacy')">前往设置</button>
    </div>
    <template v-else>
      <div v-if="loading && !data" class="empty-state" role="status">
        <span class="spinner" /> 正在加载首页…
      </div>
      <div v-if="error" class="discovery-home-error" role="status">
        {{ error }}
        <button class="text-button" type="button" :disabled="loading" @click="model.refresh">
          重试
        </button>
      </div>
      <div v-if="data?.recommendationEnabled && !blocks.length" class="empty-state">
        暂无推荐内容
      </div>
      <template v-for="section in blocks" :key="section === 'rankings' ? section : section.kind">
        <section
          v-if="section === 'rankings' && ranking"
          class="discovery-home-block"
          data-home-section="rankings"
          aria-label="榜单精选"
          @focusin="rememberRankingFocus"
        >
          <div class="discovery-home-block-heading">
            <h2>
              <button
                class="discovery-home-section-link"
                type="button"
                data-home-card="rankings:home"
                @click="emit('rankings')"
              >
                榜单精选 <Icon name="back" />
              </button>
            </h2>
          </div>
          <RankingCards
            :groups="[ranking]"
            :playable="playable"
            :current-playlist-id="currentPlaylistId"
            :playback-state="playbackState"
            focus-key-prefix="rankings:card:"
            hide-group-heading
            @open="(card) => emit('openRank', card)"
            @play="(id) => emit('playPlaylist', id)"
          />
        </section>
        <section
          v-else-if="section !== 'rankings'"
          class="discovery-home-block"
          :data-home-section="section.kind"
          :aria-label="section.title"
        >
          <div class="discovery-home-block-heading">
            <h2>
              <button
                v-if="section.kind === 'playlists' || section.kind === 'radar'"
                class="discovery-home-section-link"
                type="button"
                :data-home-card="section.kind + ':square'"
                @focus="viewport.focus = section.kind + ':square'"
                @click="emit('playlistSquare')"
              >
                {{ section.title }} <Icon name="back" />
              </button>
              <template v-else>{{ section.title }}</template>
            </h2>
            <div
              v-if="railBounds.get(section.kind)?.previous || railBounds.get(section.kind)?.next"
            >
              <button
                class="icon-button"
                type="button"
                :aria-label="`${section.title}上一组`"
                :aria-disabled="!railBounds.get(section.kind)?.previous"
                @click="scrollRail($event, section, -1)"
              >
                <Icon name="back" />
              </button>
              <button
                class="icon-button discovery-home-next"
                type="button"
                :aria-label="`${section.title}下一组`"
                :aria-disabled="!railBounds.get(section.kind)?.next"
                @click="scrollRail($event, section, 1)"
              >
                <Icon name="back" />
              </button>
            </div>
          </div>
          <div
            class="discovery-home-rail"
            :class="{
              'discovery-home-compact': section.cards.every(
                (card) => card.target.kind === 'song' || card.target.kind === 'voice',
              ),
            }"
            :data-home-rail="section.kind"
            @scroll.passive="railScrolled"
          >
            <div
              v-for="card in section.cards"
              :key="key(section, card)"
              class="discovery-home-tile"
            >
              <button
                class="discovery-home-card"
                type="button"
                :data-home-card="key(section, card)"
                :aria-label="`${card.target.kind === 'song' || card.target.kind === 'voice' ? '播放 ' : '打开 '}${card.title}`"
                :disabled="
                  !playable && (card.target.kind === 'song' || card.target.kind === 'voice')
                "
                @focus="viewport.focus = key(section, card)"
                @click="emit('open', card, section)"
              >
                <span
                  class="discovery-home-art"
                  :class="{ 'discovery-home-daily': card.target.kind === 'daily' }"
                >
                  <img
                    v-if="card.coverUrl && !failed.has(card.coverUrl)"
                    :src="card.coverUrl"
                    alt=""
                    loading="lazy"
                    decoding="async"
                    referrerpolicy="no-referrer"
                    @error="card.coverUrl && failed.add(card.coverUrl)"
                  />
                  <span v-else-if="card.target.kind === 'daily'" aria-hidden="true">{{ day }}</span>
                  <Icon v-else name="music" />
                </span>
                <span class="discovery-home-copy">
                  <strong class="ellipsis">{{ card.title }}</strong>
                  <span v-if="card.subtitle && card.subtitle !== card.title" class="ellipsis">{{
                    card.subtitle
                  }}</span>
                </span>
              </button>
              <button
                v-if="card.target.kind === 'playlist' && 'id' in card.target"
                class="icon-button discovery-home-play"
                type="button"
                :aria-label="`播放歌单 ${card.title}`"
                :disabled="!playable"
                @click="emit('playPlaylist', card.target.id)"
              >
                <Icon name="play" />
              </button>
            </div>
          </div>
        </section>
      </template>
    </template>
  </section>
</template>
