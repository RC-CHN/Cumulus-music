<script setup lang="ts">
import {
  computed,
  nextTick,
  onBeforeUnmount,
  onMounted,
  reactive,
  ref,
  shallowRef,
  watch,
} from 'vue';
import type { OrdinaryEssentialModel } from '../composables/useOrdinaryEssential';
import { useMotion } from '../composables/useMotion';
import type {
  EssentialCard,
  EssentialCategory,
  EssentialRail,
} from '../services/ordinaryEssential';
import Icon from './Icon.vue';
import ActionMenu from './ActionMenu.vue';
import type { MenuItem } from '../services/menu';
import type { SongDownloadState } from '../services/downloads';

const props = defineProps<{
  model: OrdinaryEssentialModel;
  authenticated: boolean;
  playable: boolean;
  playlistItems: (card: EssentialCard) => MenuItem[];
  voiceQueueable: boolean;
  voiceFeedback: string;
  voiceDownloadState: (card: EssentialCard) => Promise<SongDownloadState>;
  playing: boolean;
  track?: string;
  playlist?: string;
  voice?: string;
  radio?: string;
}>();
const emit = defineEmits<{
  login: [];
  section: [rail: EssentialRail];
  open: [rail: EssentialRail, card: EssentialCard];
  play: [rail: EssentialRail, card: EssentialCard, ids: string[]];
  playlistAction: [card: EssentialCard, key: string];
  voiceAction: [card: EssentialCard, key: 'next' | 'download', anchor: HTMLElement];
}>();
const { data, category, loading, changing, error, categoryError, viewport, online } = props.model;
const root = ref<HTMLElement>();
const actionMenu = ref<InstanceType<typeof ActionMenu>>();
const menuTarget = shallowRef<{
  card: EssentialCard;
  rail: 'playlists' | 'voices';
  anchor: HTMLButtonElement;
}>();
const downloadState = ref<SongDownloadState>();
const menuItems = computed<MenuItem[]>(() => {
  const target = menuTarget.value;
  if (!target) return [];
  if (target.rail === 'playlists') return props.playlistItems(target.card);
  return [
    { key: 'next', label: '下一首播放', disabled: !props.voiceQueueable },
    ...(downloadState.value === 'complete'
      ? []
      : [{ key: 'download', label: '下载', icon: 'download' }]),
  ];
});
function cardContext(
  event: MouseEvent | KeyboardEvent,
  rail: EssentialRail,
  card: EssentialCard,
  more = false,
) {
  if (
    (rail !== 'playlists' && rail !== 'voices') ||
    (event instanceof KeyboardEvent &&
      event.key !== 'ContextMenu' &&
      !(event.key === 'F10' && event.shiftKey))
  )
    return;
  const row = event.currentTarget as HTMLElement;
  const anchor = more
    ? (row as HTMLButtonElement)
    : (event.target instanceof Element &&
        event.target.closest<HTMLButtonElement>('button:not(:disabled)')) ||
      row.querySelector<HTMLButtonElement>(
        rail === 'voices' ? '.essential-voice-more' : '.essential-card-title',
      );
  if (!anchor) return;
  event.preventDefault();
  event.stopPropagation();
  const target = { card, anchor, rail } as const;
  menuTarget.value = target;
  downloadState.value = undefined;
  if (rail === 'voices') {
    void props.voiceDownloadState(card).then(
      (state) => {
        if (menuTarget.value === target) downloadState.value = state;
      },
      () => {}, // The explicit action can retry the local status lookup.
    );
  }
  const rect = anchor.getBoundingClientRect();
  const pointer = !more && event instanceof MouseEvent && (event.clientX || event.clientY);
  void actionMenu.value?.showAt(
    pointer ? event.clientX : rect.left + rect.width / 2,
    pointer ? event.clientY : more ? rect.bottom : rect.top + rect.height / 2,
    anchor,
  );
}
function menuAction(key: string) {
  const current = menuTarget.value;
  if (!current?.anchor.isConnected || current.anchor.closest('.page-leave-active')) return;
  if (current.rail === 'playlists') emit('playlistAction', current.card, key);
  else if (key === 'next' || key === 'download')
    emit('voiceAction', current.card, key, current.anchor);
}
function directVoiceDownload(event: MouseEvent, card: EssentialCard) {
  const anchor = event.currentTarget as HTMLButtonElement;
  anchor.focus({ preventScroll: true });
  emit('voiceAction', card, 'download', anchor);
}
function activatePlaylist(event: MouseEvent, rail: EssentialRail, card: EssentialCard) {
  if (rail !== 'playlists' || !(event.target instanceof Element) || event.target.closest('button'))
    return;
  const trigger = (event.currentTarget as HTMLElement).querySelector<HTMLButtonElement>(
    '.essential-card-title',
  );
  trigger?.focus({ preventScroll: true });
  emit('open', rail, card);
}

const motion = useMotion();
const geometry = reactive(
  new Map<EssentialRail, { columns: number; previous: boolean; next: boolean }>(),
);
let observer: ResizeObserver | undefined;
let restoration = 0;
let restoring = false;
let releaseTimer: number | undefined;
const unavailableCovers = ref(new Set<string>());
const categories = ['全部分类', '男生小说', '女生小说', '有声广播剧', '文学出版'];

interface Rail {
  kind: EssentialRail;
  heading: string;
  groups: EssentialCard[][];
}
function groups(cards: EssentialCard[], size: number): EssentialCard[][] {
  const full = cards.length - (cards.length % size);
  return Array.from({ length: full / size }, (_, index) =>
    cards.slice(size * index, size * index + size),
  );
}
const rails = computed<Rail[]>(() => {
  const content = data.value?.content;
  if (!content) return [];
  const rows: Rail[] = [
    { kind: 'playlists', heading: '官方歌单', groups: groups(content.playlists, 1) },
    { kind: 'songs', heading: '最新音乐', groups: groups(content.songs, 3) },
    {
      kind: 'books',
      heading: content.books?.title ?? '精选有声书',
      groups: groups(content.books?.cards ?? [], 3),
    },
    { kind: 'voices', heading: '热门播客', groups: groups(content.voices, 3) },
  ];
  return rows.filter((rail) => rail.groups.length >= (rail.kind === 'playlists' ? 1 : 2));
});
function measure(el: HTMLElement) {
  const kind = el.dataset.essentialRail as EssentialRail;
  const span = kind === 'playlists' ? 3 : 6;
  const columns = Math.max(
    1,
    Math.min(Math.floor(18 / span), Math.floor((el.clientWidth + 20) / (span * 66))),
  );
  const value = {
    columns,
    previous: el.scrollLeft > 1,
    next: el.scrollWidth - el.clientWidth - el.scrollLeft > 1,
  };
  const previous = geometry.get(kind);
  if (
    previous?.columns !== columns ||
    previous.previous !== value.previous ||
    previous.next !== value.next
  )
    geometry.set(kind, value);
}
async function observe() {
  await nextTick();
  observer?.disconnect();
  root.value?.querySelectorAll<HTMLElement>('[data-essential-rail]').forEach((el) => {
    observer?.observe(el);
    for (const column of el.children) observer?.observe(column);
    measure(el);
  });
}
function move(rail: EssentialRail, direction: number) {
  const el = root.value?.querySelector<HTMLElement>(`[data-essential-rail="${rail}"]`);
  if (!el) return;
  el.scrollBy({
    left: direction * el.clientWidth,
    behavior: motion.reduced.value ? 'instant' : 'smooth',
  });
}
function railScroll(event: Event, kind: EssentialRail) {
  const el = event.currentTarget as HTMLElement;
  if (!restoring) viewport.rails.set(kind, el.scrollLeft);
  measure(el);
}
function storeFocus(event: FocusEvent) {
  const key = (event.target as HTMLElement).dataset.essentialFocus;
  if (key) viewport.focus = key;
}
async function restore(focus = true) {
  const request = ++restoration;
  const saved = { scroll: viewport.scroll, rails: new Map(viewport.rails) };
  restoring = true;
  // Measure the responsive columns before applying saved offsets. Restoring
  // against the first-paint columns can cause scroll snap to select another page.
  await observe();
  await nextTick();
  if (!root.value || request !== restoration) return;
  root.value.scrollTop = saved.scroll;
  for (const el of root.value.querySelectorAll<HTMLElement>('[data-essential-rail]')) {
    el.scrollLeft = saved.rails.get(el.dataset.essentialRail as EssentialRail) ?? 0;
    measure(el);
  }
  const finish = () => {
    if (request !== restoration) return;
    restoring = false;
    window.clearTimeout(releaseTimer);
  };
  window.clearTimeout(releaseTimer);
  releaseTimer = window.setTimeout(finish, 150);
  requestAnimationFrame(finish);
  const active = document.activeElement;
  if (focus && (active === document.body || active?.getAttribute('aria-label') === '返回上一页')) {
    for (const el of root.value.querySelectorAll<HTMLElement>('[data-essential-focus]')) {
      if (el.dataset.essentialFocus === viewport.focus) {
        el.focus({ preventScroll: true });
        break;
      }
    }
  }
}
function select(event: Event) {
  const el = event.currentTarget as HTMLSelectElement;
  const next = Number(el.value) as EssentialCategory;
  el.value = String(category.value);
  void props.model.select(next);
}
function active(rail: EssentialRail, card: EssentialCard) {
  if (rail === 'songs') return props.track === card.id;
  if (rail === 'playlists') return props.playlist === card.id;
  if (rail === 'voices') return props.voice === card.id;
  return props.radio === card.id;
}
function canOpen(rail: EssentialRail, card: EssentialCard) {
  return rail === 'playlists' || rail === 'books' || (rail === 'voices' && !!card.radioId);
}
function play(rail: Rail, card: EssentialCard) {
  if (
    !props.playable ||
    card.history ||
    (rail.kind === 'playlists' && card.playlistMenu?.hasTracks === false)
  )
    return;
  emit(
    'play',
    rail.kind,
    card,
    rail.groups.flat().map((entry) => entry.id),
  );
}
watch(category, () => void restore(false), { flush: 'post' });
watch(rails, () => void observe(), { flush: 'post' });
onMounted(() => {
  observer = new ResizeObserver(() => {
    root.value?.querySelectorAll<HTMLElement>('[data-essential-rail]').forEach(measure);
  });
  void restore();
});
onBeforeUnmount(() => {
  restoration++;
  window.clearTimeout(releaseTimer);
  observer?.disconnect();
});
defineExpose({ restore });
</script>

<template>
  <div
    ref="root"
    class="ordinary-essential"
    aria-label="云音乐精选"
    @focusin="storeFocus"
    @scroll="!restoring && (viewport.scroll = ($event.currentTarget as HTMLElement).scrollTop)"
  >
    <div v-if="!authenticated" class="empty-state">
      <p>登录后查看精选内容</p>
      <button class="text-button" type="button" @click="emit('login')">登录账号</button>
    </div>
    <div v-else-if="loading && !data" class="empty-state" role="status">
      <span class="spinner" />
      <p>正在加载精选…</p>
    </div>
    <div v-else-if="error" class="empty-state" role="status">
      <p>{{ error }}</p>
      <button class="text-button" type="button" @click="model.retry">重试</button>
    </div>
    <div v-else-if="!online && !data" class="empty-state" role="status">当前未连接网络</div>
    <div v-else-if="data && !rails.length" class="empty-state" role="status">暂无精选内容</div>
    <section v-for="rail in error ? [] : rails" :key="rail.kind" :class="`essential-${rail.kind}`">
      <header>
        <h2 v-if="rail.kind === 'books'">{{ rail.heading }}</h2>
        <h2 v-else>
          <button
            class="section-link"
            type="button"
            :data-essential-focus="`${rail.kind}:heading`"
            @click="emit('section', rail.kind)"
          >
            {{ rail.heading }}
            <Icon name="forward" />
          </button>
        </h2>
        <select
          v-if="rail.kind === 'books'"
          :value="category"
          :disabled="loading || changing"
          :aria-busy="changing"
          aria-label="有声书分类"
          data-essential-focus="books:category"
          @change="select"
        >
          <option v-for="(label, index) in categories" :key="label" :value="index">
            {{ label }}
          </option>
        </select>
        <div
          v-if="geometry.get(rail.kind)?.previous || geometry.get(rail.kind)?.next"
          class="rail-arrows"
        >
          <button
            type="button"
            :aria-label="`${rail.heading}上一组`"
            :disabled="!geometry.get(rail.kind)?.previous"
            @click="move(rail.kind, -1)"
          >
            ‹
          </button>
          <button
            type="button"
            :aria-label="`${rail.heading}下一组`"
            :disabled="!geometry.get(rail.kind)?.next"
            @click="move(rail.kind, 1)"
          >
            ›
          </button>
        </div>
      </header>
      <p v-if="rail.kind === 'books' && categoryError" role="status">{{ categoryError }}</p>
      <div
        class="essential-rail"
        :data-essential-rail="rail.kind"
        :style="{ '--choice-columns': geometry.get(rail.kind)?.columns ?? 2 }"
        @scroll="railScroll($event, rail.kind)"
      >
        <div v-for="column in rail.groups" :key="column[0]?.id" class="essential-column">
          <article
            v-for="card in column"
            :key="card.id"
            class="essential-card"
            :data-essential-card="`${rail.kind}:${card.id}`"
            @click="activatePlaylist($event, rail.kind, card)"
            @contextmenu="cardContext($event, rail.kind, card)"
            @keydown="cardContext($event, rail.kind, card)"
            @dblclick="(rail.kind === 'songs' || rail.kind === 'voices') && play(rail, card)"
          >
            <div class="essential-artwork">
              <img
                v-if="card.coverUrl && !unavailableCovers.has(card.coverUrl)"
                :src="card.coverUrl"
                alt=""
                loading="lazy"
                @error="unavailableCovers.add(card.coverUrl)"
              />
              <Icon v-else :name="card.history ? 'history' : 'music'" />
              <button
                v-if="!card.history"
                class="essential-play"
                type="button"
                :aria-label="`${active(rail.kind, card) && playing ? '暂停' : '播放'}${card.title}`"
                :data-essential-focus="`${rail.kind}:${card.id}:play`"
                :disabled="
                  !playable || (rail.kind === 'playlists' && card.playlistMenu?.hasTracks === false)
                "
                :aria-haspopup="rail.kind === 'playlists' ? 'menu' : undefined"
                @click.stop="play(rail, card)"
                @dblclick.stop
              >
                <Icon :name="active(rail.kind, card) && playing ? 'pause' : 'play'" />
              </button>
            </div>
            <div class="essential-labels">
              <button
                v-if="rail.kind !== 'voices' && canOpen(rail.kind, card)"
                class="essential-card-title"
                type="button"
                :data-essential-focus="`${rail.kind}:${card.id}:open`"
                :aria-haspopup="rail.kind === 'playlists' ? 'menu' : undefined"
                @click="emit('open', rail.kind, card)"
                @dblclick.stop
              >
                {{ card.title }}
              </button>
              <span v-else class="essential-card-title">{{ card.title }}</span>
              <small v-if="card.subtitle">
                <button
                  v-if="rail.kind === 'voices' && card.radioId"
                  class="essential-card-title"
                  type="button"
                  :data-essential-focus="`${rail.kind}:${card.id}:open`"
                  @click="emit('open', rail.kind, card)"
                  @dblclick.stop
                >
                  {{ card.subtitle }}
                </button>
                <template v-else>{{ card.subtitle }}</template>
              </small>
            </div>
            <div v-if="rail.kind === 'voices'" class="essential-voice-actions">
              <button
                type="button"
                :aria-label="`下载${card.title}`"
                :data-essential-focus="`voices:${card.id}:download`"
                @click.stop="directVoiceDownload($event, card)"
                @dblclick.stop
              >
                <Icon name="download" />
              </button>
              <button
                class="essential-voice-more"
                type="button"
                :aria-label="`${card.title}的更多操作`"
                :data-essential-focus="`voices:${card.id}:menu`"
                aria-haspopup="menu"
                aria-expanded="false"
                @click.stop="cardContext($event, rail.kind, card, true)"
                @dblclick.stop
              >
                <Icon name="more" />
              </button>
            </div>
          </article>
        </div>
      </div>
    </section>
    <Transition name="notice">
      <div v-if="voiceFeedback" class="essential-voice-feedback" role="status">
        {{ voiceFeedback }}
      </div>
    </Transition>
    <ActionMenu
      ref="actionMenu"
      context-only
      :label="`${menuTarget?.rail === 'voices' ? '播客' : '歌单'}操作：${menuTarget?.card.title ?? ''}`"
      :items="menuItems"
      @select="menuAction"
    />
  </div>
</template>

<style scoped>
.ordinary-essential {
  flex: 1;
  min-height: 0;
  overflow: auto;
  padding: 14px clamp(24px, 3.3vw, 48px) 40px;
}
section {
  margin: 0 auto 22px;
  max-width: 1200px;
}
section header {
  display: flex;
  min-height: 44px;
  gap: 12px;
  align-items: center;
}
h2 {
  margin: 0;
  color: var(--text);
  font-size: 21px;
}
button {
  color: inherit;
  background: transparent;
  border: 0;
}
button:not(:disabled):hover {
  color: var(--accent);
}
.section-link {
  display: flex;
  align-items: center;
  gap: 5px;
  padding: 0;
  font: inherit;
}
.section-link svg {
  width: 17px;
  height: 17px;
}
.rail-arrows {
  display: flex;
  margin-inline-start: auto;
  gap: 4px;
}
.rail-arrows button {
  width: 30px;
  min-height: 30px;
  border: 1px solid var(--line);
  border-radius: 50%;
  font-size: 20px;
}
select {
  min-height: 30px;
  border: 1px solid var(--line);
  border-radius: 15px;
  background: var(--surface);
  color: var(--text);
  padding-inline: 10px;
}
.essential-rail {
  display: grid;
  grid-auto-flow: column;
  grid-auto-columns: calc((100% + 20px) / var(--choice-columns) - 20px);
  gap: 20px;
  overflow: auto hidden;
  scrollbar-width: none;
  scroll-snap-type: x proximity;
  padding-bottom: 4px;
}
.essential-column {
  display: grid;
  gap: 8px;
  scroll-snap-align: start;
  min-width: 0;
}
.essential-card {
  display: flex;
  align-items: center;
  min-width: 0;
  gap: 10px;
  border-radius: 8px;
}
.essential-card:hover {
  background: var(--hover);
}
.essential-playlists .essential-card {
  display: block;
  cursor: pointer;
}
.essential-artwork {
  position: relative;
  display: grid;
  place-items: center;
  width: 56px;
  flex: none;
  aspect-ratio: 1;
  background: var(--surface-raised, var(--surface));
  border: 1px solid var(--line);
  border-radius: 8px;
  overflow: hidden;
}
.essential-artwork > svg {
  width: 23px;
  height: 23px;
  color: var(--text-muted);
}
.essential-playlists .essential-artwork {
  width: 100%;
}
.essential-books .essential-artwork {
  aspect-ratio: 4 / 5;
}
.essential-artwork img {
  position: absolute;
  width: 100%;
  height: 100%;
  object-fit: cover;
}
.essential-play {
  position: absolute;
  right: 5px;
  bottom: 5px;
  display: grid;
  place-items: center;
  width: 30px;
  height: 30px;
  border-radius: 50%;
  background: var(--surface);
  color: var(--text);
  opacity: 0;
}
.essential-card:hover .essential-play,
.essential-play:focus-visible,
.essential-play[aria-label^='暂停'] {
  opacity: 1;
}
.essential-play svg {
  width: 16px;
  height: 16px;
}
.essential-voice-feedback {
  position: fixed;
  bottom: 108px;
  left: 50%;
  z-index: 2;
  max-width: calc(100vw - 40px);
  padding: 10px 14px;
  border: 1px solid var(--line);
  border-radius: 10px;
  background: var(--surface);
  color: var(--text);
  pointer-events: none;
  transform: translateX(-50%);
}
.essential-voice-actions {
  display: flex;
  flex: none;
  gap: 3px;
  margin-inline-start: auto;
  opacity: 0;
}
.essential-card:hover .essential-voice-actions,
.essential-card:focus-within .essential-voice-actions,
.essential-voice-actions:has([aria-expanded='true']) {
  opacity: 1;
}
.essential-voice-actions button {
  display: grid;
  width: 26px;
  height: 28px;
  place-items: center;
  padding: 0;
}
.essential-voice-actions svg {
  width: 17px;
  height: 17px;
}
.essential-labels {
  min-width: 0;
  padding-block: 4px;
}
.essential-labels small,
.essential-card-title {
  display: block;
  max-width: 100%;
  padding: 0;
  overflow: hidden;
  color: var(--text);
  text-align: left;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.essential-labels small {
  color: var(--text-muted);
  padding-top: 3px;
}
</style>
