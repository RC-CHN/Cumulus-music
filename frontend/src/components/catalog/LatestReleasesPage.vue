<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import Icon from '../Icon.vue';
import TrackTable from '../TrackTable.vue';
import LatestAlbumGrid from './LatestAlbumGrid.vue';
import { createLatestReleaseMemory, useLatestReleases } from '../../composables/useLatestReleases';
import { useCore } from '../../services/core';
import {
  latestAlbumRegions,
  latestSongRegions,
  type LatestAlbumArea,
  type LatestReleasePosition,
  type LatestReleaseTab,
  type LatestSongArea,
  type LatestSongRowSelection,
} from '../../services/latestReleases';
import { waitForViewportAnimations, waitForViewportFrames } from '../../services/viewportLayout';
import type { AlbumCard, CatalogResource } from '../../services/catalog';
import type { AlbumCardRequest } from '../../services/catalogCards';
import type { TrackView } from '../../services/contracts';
import type {
  TrackCurrentToggle,
  TrackMenuRequest,
  TrackPlayGesture,
} from '../../services/trackMenu';
import '../../styles/latest-releases.css';

const props = defineProps<{
  epoch: string;
  visit: number;
  tab: LatestReleaseTab;
  memory: ReturnType<typeof createLatestReleaseMemory>;
  playable: boolean;
  queueable: boolean;
  liked: Set<string>;
  busyLikes: Set<string>;
  currentId?: string;
  currentToggle?: TrackCurrentToggle;
  currentAlbumId?: string;
  playing: boolean;
  observedAlbumMembership: ReadonlyMap<string, boolean>;
}>();
const emit = defineEmits<{
  tab: [tab: LatestReleaseTab];
  expired: [];
  play: [tracks: TrackView[], index: number];
  doubleClick: [selection: LatestSongRowSelection];
  collect: [tracks: TrackView[]];
  like: [track: TrackView];
  toggleCurrent: [target: TrackCurrentToggle, anchor: HTMLButtonElement];
  more: [request: TrackMenuRequest];
  open: [resource: CatalogResource];
  playAlbum: [album: AlbumCard];
  albumAction: [request: AlbumCardRequest];
}>();
const model = useLatestReleases(useCore(), props.memory, () => emit('expired'));
const {
  songArea,
  songPage,
  playable,
  albumArea,
  recommended,
  albumGroups,
  next,
  pending,
  error,
  ready,
  autoMore,
} = model;
const tabs = [
  ['songs', '新歌速递'],
  ['albums', '新碟上架'],
] as const;
const root = ref<HTMLElement>();
const table = ref<InstanceType<typeof TrackTable>>();
const grid = ref<InstanceType<typeof LatestAlbumGrid>>();
const playbackNotice = ref(false);
const eligibility = computed(() => new Set(playable.value.map((track) => track.id)));
const collectible = computed(
  () =>
    songPage.value?.tracks.filter(
      (track) => !track.missing && !track.local && /^[1-9][0-9]*$/.test(track.id),
    ) ?? [],
);
let saved: LatestReleasePosition | undefined;
let alive = true;
let restored = false;
let restoring = false;
let moved = false;
let initialFocus: Element | null = null;
let lastFocus: string | undefined;
let recommendationTimer: ReturnType<typeof setTimeout> | undefined;
const scroller = () => root.value?.querySelector<HTMLElement>('[data-latest-scroll],.track-scroll');

function focusKey(node: EventTarget | null) {
  if (!(node instanceof HTMLElement) || !root.value?.contains(node)) return;
  const card = node.closest<HTMLElement>('[data-catalog-key]');
  if (card)
    return `album:${card.closest<HTMLElement>('[data-latest-group]')?.dataset.latestGroup}:${card.dataset.catalogKey}`;
  const track = node.closest<HTMLElement>('[data-track-id]');
  if (track)
    return `track:${track.dataset.trackId}:${node.getAttribute('aria-label') ?? node.textContent?.trim() ?? ''}`;
  return node.dataset.latestFocus;
}
function remember(event: FocusEvent) {
  lastFocus = focusKey(event.target) ?? lastFocus;
}
function rememberPointer(event: PointerEvent) {
  if (event.target === scroller()) markMoved();
  if (event.target instanceof Element)
    lastFocus = focusKey(event.target.closest('button')) ?? lastFocus;
}
function markMoved() {
  moved = true;
}
function markKeyboardScroll(event: KeyboardEvent) {
  const target = event.target;
  if (
    target instanceof Node &&
    scroller()?.contains(target) &&
    ['PageUp', 'PageDown', 'Home', 'End', 'ArrowUp', 'ArrowDown'].includes(event.key)
  )
    markMoved();
}
function cancelRecommendation() {
  clearTimeout(recommendationTimer);
  recommendationTimer = undefined;
}
function checkpoint() {
  if (!alive) return;
  alive = false;
  cancelRecommendation();
  model.leave({
    top: scroller()?.scrollTop ?? saved?.top ?? 0,
    focus: lastFocus ?? focusKey(document.activeElement) ?? saved?.focus,
  });
}
defineExpose({ checkpoint });

async function restore() {
  const element = root.value;
  if (!alive || !element || restoring || restored || (!ready.value && !error.value)) return;
  restoring = true;
  const valid = () =>
    alive && root.value === element && element.isConnected && !element.closest('[inert]');
  try {
    await nextTick();
    await waitForViewportFrames();
    if (!valid()) return;
    await waitForViewportAnimations(element.closest('.page-view'), false);
    if (!valid()) return;
    if (saved && !moved) {
      if (props.tab === 'songs') table.value?.restoreScroll(saved.top);
      else grid.value?.restoreScroll(saved.top);
    }
    await nextTick();
    if (!valid()) return;
    restored = true;
    const current = document.activeElement;
    if (current !== document.body && current !== initialFocus && !element.contains(current)) return;
    if (element.contains(current)) return;
    const control = saved?.focus
      ? [...element.querySelectorAll<HTMLElement>('button:not(:disabled),[tabindex="-1"]')].find(
          (candidate) => focusKey(candidate) === saved?.focus,
        )
      : undefined;
    (control ?? element.querySelector<HTMLElement>('[role="tab"][aria-selected="true"]'))?.focus({
      preventScroll: true,
    });
  } finally {
    restoring = false;
  }
}
onMounted(() => {
  initialFocus = document.activeElement;
  saved = model.open(props.epoch, props.visit, props.tab);
  void restore();
});
watch([ready, error], () => void restore(), { flush: 'post' });
onBeforeUnmount(checkpoint);

function changeTab(tab: LatestReleaseTab) {
  if (alive && props.tab !== tab) emit('tab', tab);
}
function region(area: LatestSongArea | LatestAlbumArea) {
  cancelRecommendation();
  if (props.tab === 'songs') {
    playbackNotice.value = false;
    if (model.chooseSong(area as LatestSongArea)) table.value?.restoreScroll(0);
  } else if (model.chooseAlbum(area as LatestAlbumArea)) grid.value?.restoreScroll(0);
}
function recommendation(value: boolean) {
  cancelRecommendation();
  recommendationTimer = setTimeout(() => {
    if (alive && model.chooseAlbum(albumArea.value, value)) grid.value?.restoreScroll(0);
  }, 300);
}
function play(index: number, gesture?: TrackPlayGesture) {
  const page = songPage.value;
  const track = page?.tracks[index];
  if (!alive || !props.playable || !page || !track || !eligibility.value.has(track.id)) return;
  playbackNotice.value = false;
  if (gesture === 'double_click') {
    emit('doubleClick', { area: songArea.value, revision: page.revision, start_id: track.id });
  } else {
    emit(
      'play',
      playable.value,
      playable.value.findIndex((row) => row.id === track.id),
    );
  }
}
function playAll() {
  const first = songPage.value?.tracks[0];
  if (!alive || !props.playable || !first) return;
  // The bulk action assigns the original first track, before filtering the list.
  if (!eligibility.value.has(first.id)) {
    playbackNotice.value = true;
    return;
  }
  play(0);
}
function toggleCurrent(target: TrackCurrentToggle, anchor: HTMLButtonElement) {
  if (!alive) return;
  playbackNotice.value = false;
  emit('toggleCurrent', target, anchor);
}
function more(request: TrackMenuRequest) {
  if (alive)
    emit('more', { ...request, playUnavailable: !eligibility.value.has(request.track.id) });
}
</script>

<template>
  <section
    ref="root"
    class="latest-releases-page"
    data-latest-releases
    @focusin="remember"
    @pointerdown.capture="rememberPointer"
    @keydown.capture="markKeyboardScroll"
    @wheel.passive="markMoved"
    @touchmove.passive="markMoved"
  >
    <div class="latest-release-tabs" role="tablist" aria-label="新歌新碟">
      <button
        v-for="[key, title] in tabs"
        :id="`latest-release-tab-${visit}-${key}`"
        :key="key"
        role="tab"
        type="button"
        :tabindex="tab === key ? 0 : -1"
        :aria-selected="tab === key"
        :aria-controls="tab === key ? `latest-release-panel-${visit}-${tab}` : undefined"
        :data-latest-focus="`tab:${key}`"
        @click="changeTab(key)"
        @keydown.left.prevent="changeTab(key === 'songs' ? 'albums' : 'songs')"
        @keydown.right.prevent="changeTab(key === 'songs' ? 'albums' : 'songs')"
      >
        {{ title }}
      </button>
    </div>
    <div
      :id="`latest-release-panel-${visit}-${tab}`"
      class="latest-release-panel"
      role="tabpanel"
      :aria-labelledby="`latest-release-tab-${visit}-${tab}`"
      :aria-busy="pending"
    >
      <div class="latest-release-toolbar">
        <div class="latest-release-regions" role="group" aria-label="地区">
          <button
            v-for="[key, title] in tab === 'songs' ? latestSongRegions : latestAlbumRegions"
            :key="key"
            type="button"
            :aria-pressed="key === (tab === 'songs' ? songArea : albumArea)"
            :aria-disabled="tab === 'albums' && pending ? true : undefined"
            :data-latest-focus="`region:${key}`"
            @click="region(key)"
          >
            {{ title }}
          </button>
        </div>
        <div v-if="tab === 'songs'" class="latest-release-actions">
          <button
            class="primary"
            type="button"
            :disabled="!songPage?.tracks.length || !props.playable"
            data-latest-focus="play-all"
            @click="playAll"
          >
            <Icon name="play" /> 播放全部
          </button>
          <button
            class="secondary"
            type="button"
            :disabled="!collectible.length"
            data-latest-focus="collect-all"
            @click="emit('collect', collectible)"
          >
            <Icon name="star" /> 收藏全部
          </button>
        </div>
        <div v-else class="latest-release-recommendations" role="group" aria-label="新碟筛选">
          <button
            v-for="[value, label] in [
              [false, '全部'],
              [true, '推荐'],
            ] as const"
            :key="label"
            class="text-button"
            type="button"
            :aria-pressed="recommended === value"
            :aria-disabled="pending ? true : undefined"
            :data-latest-focus="`recommendation:${value}`"
            @click="!pending && recommendation(value)"
          >
            {{ label }}
          </button>
        </div>
      </div>
      <template v-if="tab === 'songs'">
        <p
          v-if="playbackNotice"
          class="latest-song-feedback latest-playback-notice"
          data-latest-playback-notice
          role="status"
        >
          第一首歌曲当前无法播放，请选择列表中的其他歌曲。
        </p>
        <div v-if="error && songPage" class="latest-song-feedback" role="alert">
          <span>{{ error }}</span>
          <button class="text-button" type="button" @click="model.retry()">重试</button>
        </div>
        <div v-if="pending && !songPage" class="empty-state" role="status">
          <span class="spinner" />
          <p>正在加载新歌…</p>
        </div>
        <div v-else-if="error && !songPage" class="empty-state" role="alert">
          <p>{{ error }}</p>
          <button class="text-button" type="button" @click="model.retry()">重试</button>
        </div>
        <TrackTable
          v-else-if="songPage?.tracks.length"
          :key="songArea"
          ref="table"
          :tracks="songPage.tracks"
          :playable-ids="eligibility"
          :liked="liked"
          :busy="busyLikes"
          :playable="props.playable"
          :current-id="currentId"
          :current-toggle="currentToggle"
          preserve-scroll-on-refresh
          navigable
          @play="play"
          @like="emit('like', $event)"
          @toggle-current="toggleCurrent"
          @more="more"
          @artist="(id) => emit('open', { kind: 'artist', id })"
          @album="(id) => emit('open', { kind: 'album', id })"
        />
        <p v-else class="empty-state">暂无新歌</p>
      </template>
      <LatestAlbumGrid
        v-else
        ref="grid"
        :groups="albumGroups"
        :playable="props.playable"
        :queueable="queueable"
        :current-album-id="currentAlbumId"
        :playing="playing"
        :observed-album-membership="observedAlbumMembership"
        :pending="pending"
        :more="next !== null"
        :auto-more="autoMore"
        :error="error"
        @open="emit('open', $event)"
        @play="emit('playAlbum', $event)"
        @action="emit('albumAction', $event)"
        @more="model.load(true)"
        @retry="model.retry()"
      />
    </div>
  </section>
</template>
