<script setup lang="ts">
import BoundedPanelTransition from './BoundedPanelTransition.vue';
import { computed, nextTick, onBeforeUnmount, onMounted, ref, useId, watch } from 'vue';
import type { TrackCurrentToggle, TrackMenuRequest, TrackPlayGesture } from '../services/trackMenu';
import type { AccountView, TrackView } from '../services/contracts';
import type { ArtistTab, CatalogResource, CatalogSelectionPage } from '../services/catalog';
import type { AlbumCard, ArtistCard } from '../services/catalog';
import type { AlbumCardRequest } from '../services/catalogCards';
import type { ArtistCardRequest } from '../services/artistCards';
import type { AlbumQueueDropGesture, PlayerOrigin, PlayerView } from '../services/player';
import type { AlbumPlaybackScope, AlbumPlaybackRow } from '../services/albumDurationSort';
import type { AlbumTab, CatalogPanelKey } from '../services/catalogNavigation';
import type { CatalogSession } from '../composables/useCatalogSession';
import { usePageEntryFocus } from '../composables/usePageEntryFocus';
import { waitForViewportFrames } from '../services/viewportLayout';
import TrackTable from './TrackTable.vue';
import CatalogCards from './catalog/CatalogCards.vue';
import ArtistMvGrid from './catalog/ArtistMvGrid.vue';
import AlbumHeader from './catalog/AlbumHeader.vue';
import AlbumContent from './catalog/AlbumContent.vue';
import Icon from './Icon.vue';
import ArtistHeader from './catalog/ArtistHeader.vue';
import ArtistTabs from './catalog/ArtistTabs.vue';
import ActionMenu from './ActionMenu.vue';
import ModalTransition from './ModalTransition.vue';
import MenuTransition from './MenuTransition.vue';
import CatalogTrackBatchDialog from './catalog/CatalogTrackBatchDialog.vue';
const props = defineProps<{
  model: CatalogSession;
  account: AccountView;
  playable: boolean;
  playBusy: boolean;
  downloadBusy: boolean;
  liked: Set<string>;
  busyLikes: Set<string>;
  currentId?: string;
  currentToggle?: TrackCurrentToggle;
  origin?: PlayerOrigin | null;
  playing: boolean;
  queueable: boolean;
  albumQueueDropEnabled?: boolean;
  queueArtistSongs: (pages: CatalogSelectionPage[]) => Promise<PlayerView | undefined>;
  prepareArtistSongsBatch: () => (
    pages: CatalogSelectionPage[],
    ids: string[],
  ) => Promise<PlayerView | undefined>;
}>();
const emit = defineEmits<{
  open: [resource: CatalogResource];
  profile: [id: string];
  play: [pages: CatalogSelectionPage[], index: number, gesture?: TrackPlayGesture];
  playAlbumDetail: [
    album: AlbumPlaybackScope,
    row: AlbumPlaybackRow | null,
    gesture?: TrackPlayGesture,
  ];
  toggleCurrent: [target: TrackCurrentToggle, anchor: HTMLButtonElement];
  like: [track: TrackView];
  more: [request: TrackMenuRequest];
  download: [tracks: TrackView[]];
  collectTracks: [tracks: TrackView[]];
  playTracks: [tracks: TrackView[], index: number];
  mv: [id: string];
  mvComments: [id: string, title: string];
  expired: [];
  artistTab: [tab: ArtistTab];
  albumTab: [tab: AlbumTab];
  back: [];
  playAlbum: [album: AlbumCard];
  albumAction: [request: AlbumCardRequest];
  albumQueueDrag: [event: PointerEvent, gesture: AlbumQueueDropGesture];
  playArtist: [artist: ArtistCard];
  artistAction: [request: ArtistCardRequest];
}>();
const artistProfile = computed(() => {
  const page = props.model.detail.view.value?.page;
  return page?.kind === 'artist' ? page : undefined;
});
const {
  resource,
  tab,
  albumTab,
  albumOrder,
  positions,
  detail,
  dynamic,
  membership,
  list,
  artist,
  album,
  hotTracks,
  onlineCount,
  subscribed,
  stateReading,
  tracks,
  selections,
  next,
  intro,
  auxiliary,
  cards,
  artistMvAvailable,
  artistVideos,
  refreshState,
  refresh,
  collect,
} = props.model;
const batchTracks = ref<TrackView[]>();
let batchOrigin: HTMLElement | null = null;
let batchAlbumRevision: string | undefined;
let batchArtistPages: CatalogSelectionPage[] | undefined;
const batchAllowed = () =>
  props.model.valid() && (resource.kind === 'album' || tab.value === 'all');
const artistBatchAnchor = ref<HTMLElement>();
let batchHandoff:
  | {
      epoch: string;
      kind: 'play' | 'collect' | 'download' | 'queue';
      tracks: TrackView[];
      artistQueue?: () => Promise<PlayerView | undefined>;
    }
  | undefined;
function openTrackBatch(origin: HTMLElement | null) {
  if (!batchAllowed()) return;
  const usable = new Map(
    tracks.value
      .filter((track) => !track.missing && !track.local)
      .map((track) => [track.id, track]),
  );
  if (usable.size) {
    batchOrigin = origin;
    batchAlbumRevision = resource.kind === 'album' ? detail.view.value?.revision : undefined;
    batchArtistPages =
      resource.kind === 'artist' && selections.value.length ? [...selections.value] : undefined;
    batchTracks.value = [...usable.values()];
  }
}
function albumScope(): AlbumPlaybackScope | undefined {
  const current = detail.view.value;
  if (
    props.model.valid() &&
    !detail.loading.value &&
    current?.page.kind === 'album' &&
    current.query.kind === 'album' &&
    current.query.id === resource.id &&
    current.page.album.id === resource.id &&
    current.page.playbackRevision
  )
    return { id: resource.id, revision: current.page.playbackRevision, order: albumOrder.value };
}
function playCurrentAlbum(row: AlbumPlaybackRow | null, gesture?: TrackPlayGesture) {
  const scope = albumScope();
  if (scope) emit('playAlbumDetail', scope, row, gesture);
}
function queueCurrentAlbum(anchor: HTMLElement | null) {
  const scope = albumScope();
  if (!props.queueable || !scope || !album.value) return;
  emit('albumAction', { action: 'queue', album: album.value, anchor, detail: scope });
}
const artistQueueNotice = ref('');
async function artistSongsAction(key: string) {
  if (key === 'batch') {
    openTrackBatch(artistBatchAnchor.value?.querySelector('button') ?? null);
    return;
  }
  if (key === 'queue') await submitArtistQueue(() => props.queueArtistSongs(selections.value));
}
async function submitArtistQueue(submit: () => Promise<PlayerView | undefined>) {
  const epoch = props.account.epoch;
  const page = section.value;
  const current = () =>
    props.account.epoch === epoch &&
    tab.value === 'all' &&
    props.model.valid() &&
    page?.isConnected &&
    !page.closest('[inert],.page-leave-active');
  if (!props.queueable || !current()) return;
  artistQueueNotice.value = '';
  try {
    const result = await submit();
    if (result && current()) artistQueueNotice.value = '已添加到播放列表';
  } catch {
    if (current()) artistQueueNotice.value = '暂时无法添加，请确认当前歌曲列表和播放状态后重试。';
  }
}
function chooseTrackBatch(kind: 'play' | 'collect' | 'download' | 'queue', selected: TrackView[]) {
  if (!batchTracks.value || !batchAllowed()) return;
  batchHandoff = { epoch: props.account.epoch, kind, tracks: selected };
  if (kind === 'queue' && batchArtistPages) {
    const pages = batchArtistPages;
    const ids = selected.map((track) => track.id);
    const submit = props.prepareArtistSongsBatch();
    batchHandoff.artistQueue = () => submit(pages, ids);
  }
  batchTracks.value = undefined;
}
function finishTrackBatch() {
  const handoff = batchHandoff;
  batchHandoff = undefined;
  const origin = batchOrigin;
  const revision = batchAlbumRevision;
  batchOrigin = null;
  batchAlbumRevision = undefined;
  batchArtistPages = undefined;
  if (!handoff || handoff.epoch !== props.account.epoch || !batchAllowed()) return;
  if (handoff.kind === 'queue') {
    if (resource.kind === 'album' && props.queueable && album.value && revision)
      emit('albumAction', {
        action: 'queue',
        album: album.value,
        anchor: origin,
        selection: { ids: handoff.tracks.map((track) => track.id), revision },
      });
    else if (handoff.artistQueue) void submitArtistQueue(handoff.artistQueue);
  } else if (handoff.kind === 'collect') emit('collectTracks', handoff.tracks);
  else if (handoff.kind === 'download') emit('download', handoff.tracks);
  else if (props.playable && !props.playBusy) emit('playTracks', handoff.tracks, 0);
}
watch(
  [tab, albumTab, () => props.account.epoch],
  () => {
    batchHandoff = undefined;
    batchTracks.value = undefined;
    artistQueueNotice.value = '';
  },
  { flush: 'sync' },
);
const expire = () => emit('expired');
const section = ref<HTMLElement>(),
  panel = ref<HTMLElement>();
const regularAlbumHeader = ref<InstanceType<typeof AlbumHeader>>();
const albumScrolled = ref(false);
const albumSticky = computed(
  () => !!album.value && albumTab.value === 'songs' && albumScrolled.value,
);
watch(
  [section, regularAlbumHeader],
  ([root, header], _previous, onCleanup) => {
    albumScrolled.value = false;
    const button = header?.$el?.querySelector('[data-album-play]') as HTMLElement | undefined;
    if (!root || !button) return;
    const observer = new IntersectionObserver(
      ([entry]) => {
        albumScrolled.value =
          !!entry?.rootBounds && entry.boundingClientRect.bottom <= entry.rootBounds.top;
      },
      { root },
    );
    observer.observe(button);
    onCleanup(() => observer.disconnect());
  },
  { flush: 'post' },
);
watch(albumSticky, async (compact) => {
  const active = document.activeElement;
  if (
    !(active instanceof HTMLElement) ||
    !active.hasAttribute('data-album-play') ||
    !section.value?.contains(active) ||
    active.closest('.album-header')?.classList.contains('album-header-compact') === compact
  )
    return;
  await nextTick();
  if (
    albumSticky.value !== compact ||
    (document.activeElement !== active && document.activeElement !== document.body)
  )
    return;
  const header = compact ? '.album-header-compact' : '.album-header:not(.album-header-compact)';
  section.value
    ?.querySelector<HTMLButtonElement>(`${header} [data-album-play]:not(:disabled)`)
    ?.focus({ preventScroll: true });
});
const panelId = useId();
const selectedPanel = computed<CatalogPanelKey>(() =>
  resource.kind === 'artist' ? `artist:${tab.value}` : `album:${albumTab.value}`,
);
const entryPanel = selectedPanel.value;
usePageEntryFocus(
  section,
  () => !!detail.view.value || !!detail.error.value,
  () =>
    props.model.valid() &&
    selectedPanel.value === entryPanel &&
    !positions.has(entryPanel) &&
    !(entryPanel === 'album:comments' && props.model.comments.initialTarget.value),
);
let pendingRestore: CatalogPanelKey | undefined = positions.has(selectedPanel.value)
  ? selectedPanel.value
  : undefined;
let restoring = 0;
function panelKey(element: Element): CatalogPanelKey | undefined {
  const node = element as HTMLElement;
  return node.dataset.artistPanel
    ? `artist:${node.dataset.artistPanel as ArtistTab}`
    : node.dataset.albumPanel
      ? `album:${node.dataset.albumPanel as AlbumTab}`
      : undefined;
}
function currentPanel(key = selectedPanel.value) {
  return resource.kind === 'artist'
    ? panel.value
    : section.value?.querySelector<HTMLElement>(`[data-album-panel="${key.slice(6)}"]`);
}
function checkpoint(key = selectedPanel.value) {
  if (!props.model.valid()) return;
  const node = currentPanel(key);
  if (!node || panelKey(node) !== key || pendingRestore === key) return;
  const inner = node.querySelector<HTMLElement>('.track-scroll,.catalog-card-scroll');
  const active = document.activeElement;
  const focus =
    active instanceof HTMLElement && section.value?.contains(active)
      ? active.closest<HTMLElement>('[data-catalog-key]')?.dataset.catalogKey
      : positions.get(key)?.focus;
  positions.set(key, { inner: inner?.scrollTop ?? 0, outer: section.value?.scrollTop ?? 0, focus });
}
function cancelRestore(event?: Event) {
  if (event instanceof KeyboardEvent && (event.target as Element)?.closest('.catalog-tabs')) return;
  pendingRestore = undefined;
  restoring++;
}
async function restore(element: Element) {
  const owner = selectedPanel.value,
    saved = positions.get(owner),
    ticket = ++restoring;
  if (pendingRestore !== owner || !saved || panelKey(element) !== owner) return;
  const valid = () =>
    props.model.valid() &&
    ticket === restoring &&
    selectedPanel.value === owner &&
    element.isConnected &&
    panelKey(element) === owner;
  await waitForViewportFrames(2);
  if (!valid() || pendingRestore !== owner) return;
  const inner = element.querySelector<HTMLElement>('.track-scroll,.catalog-card-scroll');
  if (inner) {
    inner.scrollTop = saved.inner;
    inner.dispatchEvent(new Event('scroll'));
  }
  if (section.value) section.value.scrollTop = saved.outer;
  const returnTarget = document.activeElement;
  pendingRestore = undefined;
  if (
    !saved.focus ||
    (returnTarget !== document.body && !returnTarget?.classList.contains('back-button'))
  )
    return;
  await waitForViewportFrames();
  await nextTick();
  if (valid() && document.activeElement === returnTarget)
    element
      .querySelector<HTMLElement>(`[data-catalog-key="${CSS.escape(saved.focus)}"]`)
      ?.focus({ preventScroll: true });
}
watch(
  selectedPanel,
  (next, previous) => {
    checkpoint(previous);
    restoring++;
    pendingRestore = positions.has(next) ? next : undefined;
  },
  { flush: 'sync' },
);
onMounted(() => {
  void nextTick(() => {
    const node = currentPanel();
    if (node) restore(node);
  });
});
onBeforeUnmount(() => {
  checkpoint();
  cancelRestore();
});
function choose(next: ArtistTab) {
  if (next !== tab.value) emit('artistTab', next);
}
function playHot() {
  const index = hotTracks.value.findIndex((track) => !track.missing);
  if (index >= 0 && detail.view.value)
    emit('play', [{ query: detail.view.value.query, revision: detail.view.value.revision }], index);
}
function play(index: number, gesture?: TrackPlayGesture) {
  if (tracks.value[index] && !tracks.value[index].missing)
    emit('play', selections.value, index, gesture);
}
const albumControls = computed(() => ({
  digital: auxiliary.value?.digitalAlbumTag === true,
  subscribed: subscribed.value ?? null,
  collectDisabled:
    subscribed.value == null ||
    stateReading.value ||
    !membership.known.value ||
    membership.busy.value ||
    membership.pending.value,
  playDisabled:
    !props.playable ||
    props.playBusy ||
    !albumScope() ||
    !tracks.value.some((track) => !track.missing),
  downloadDisabled: props.downloadBusy || !tracks.value.some((track) => !track.missing),
  queueDisabled: !props.queueable || !albumScope(),
  batchDisabled: !tracks.value.some((track) => !track.missing && !track.local),
  refreshDisabled: detail.loading.value || membership.busy.value,
  refreshing: detail.loading.value,
}));
const albumActions = {
  open: (target: CatalogResource) => emit('open', target),
  play: () => playCurrentAlbum(null),
  collect,
  download: () =>
    emit(
      'download',
      tracks.value.filter((track) => !track.missing),
    ),
  batch: openTrackBatch,
  queue: queueCurrentAlbum,
  refresh,
};
defineExpose({ checkpoint: () => checkpoint(), resourceKey: `${resource.kind}:${resource.id}` });
</script>
<template>
  <section
    ref="section"
    class="catalog-page"
    @wheel.passive="cancelRestore"
    @touchstart.passive="cancelRestore"
    @pointerdown="cancelRestore"
    @keydown="cancelRestore"
    :aria-label="`${resource.kind === 'artist' ? '歌手' : '专辑'}详情`"
  >
    <div
      v-if="detail.error.value || (tab !== 'videos' && list.error.value)"
      class="catalog-error catalog-detail-error"
      role="alert"
    >
      <div v-if="detail.error.value">
        {{ detail.error.value }}<span v-if="detail.view.value"> · 保留上次内容</span
        ><button class="text-button" :disabled="detail.loading.value" @click="refresh">重试</button>
      </div>
      <div v-if="tab !== 'videos' && list.error.value" class="catalog-list-error">
        {{ list.error.value }}<span v-if="list.pages.value.length"> · 保留已加载内容</span
        ><button class="text-button" :disabled="list.loading.value" @click="list.retry">
          重试
        </button>
      </div>
    </div>
    <div v-if="!detail.view.value" class="empty-state" role="status">
      <span v-if="detail.loading.value" class="spinner" />{{
        detail.loading.value ? '正在加载详情…' : '暂无可显示的详情'
      }}
    </div>
    <template v-else>
      <template v-if="album">
        <div class="album-sticky-host">
          <MenuTransition>
            <AlbumHeader
              v-if="albumSticky"
              compact
              :album="album"
              v-bind="albumControls"
              v-on="albumActions"
            />
          </MenuTransition>
        </div>
        <AlbumHeader
          ref="regularAlbumHeader"
          :album="album"
          v-bind="albumControls"
          v-on="albumActions"
        />
      </template>
      <ArtistHeader
        v-else-if="artist && tab !== 'all'"
        :artist="artist"
        :profile-account-id="artistProfile?.profileAccountId"
        :is-self="artistProfile?.isSelf"
        :followed="subscribed ?? null"
        :follow-disabled="
          subscribed == null ||
          stateReading ||
          !membership.known.value ||
          membership.busy.value ||
          membership.pending.value
        "
        :play-disabled="!playable || playBusy || !hotTracks.some((track) => !track.missing)"
        :refresh-disabled="
          detail.loading.value ||
          dynamic.loading.value ||
          list.loading.value ||
          membership.busy.value
        "
        :refreshing="detail.loading.value || dynamic.loading.value"
        @profile="emit('profile', $event)"
        @play="playHot"
        @follow="collect()"
        @refresh="refresh"
      />
      <p
        v-if="detail.loading.value || dynamic.loading.value"
        class="sr-only"
        role="status"
        data-catalog-updating
      >
        正在更新详情…
      </p>
      <div v-if="dynamic.error.value || membership.error.value" class="catalog-error" role="alert">
        {{ dynamic.error.value || membership.error.value
        }}<button class="text-button" :disabled="membership.busy.value" @click="refreshState">
          重读{{ artist ? '关注' : '收藏' }}状态
        </button>
      </div>
      <div v-if="membership.pending.value" class="notice" role="status">
        上次{{ artist ? '关注' : '收藏' }}操作结果待确认<button
          class="text-button"
          :disabled="membership.busy.value"
          @click="collect(true)"
        >
          核对结果
        </button>
      </div>
      <p v-if="auxiliary && !auxiliary.privilegesRefreshed" class="notice">
        歌曲权益暂未更新，播放时将重新检查。
      </p>
      <ArtistTabs
        v-if="artist && tab !== 'all'"
        :artist="artist"
        :selected="tab"
        :online-count="onlineCount"
        :id-prefix="panelId"
        @choose="choose"
      />
      <AlbumContent
        v-if="album"
        :key="`${account.epoch}:${album.id}`"
        :album="album"
        :model="model"
        :account="account"
        :tracks="tracks"
        :liked="liked"
        :busy-likes="busyLikes"
        :playable="playable && !playBusy"
        :current-id="currentId"
        :current-toggle="currentToggle"
        :queue-drop-enabled="albumQueueDropEnabled"
        @choose="emit('albumTab', $event)"
        @enter="restore"
        @play="playCurrentAlbum"
        @toggle-current="(target, anchor) => emit('toggleCurrent', target, anchor)"
        @like="emit('like', $event)"
        @more="emit('more', $event)"
        @queue-drag="(event, gesture) => emit('albumQueueDrag', event, gesture)"
        @open="emit('open', $event)"
        @expired="expire"
      />
      <BoundedPanelTransition
        v-else
        name="catalog-tab"
        @enter="restore"
        @before-leave="($event as HTMLElement).inert = true"
        @before-enter="($event as HTMLElement).inert = false"
      >
        <div
          :id="`${panelId}-panel`"
          ref="panel"
          :data-artist-panel="tab"
          :key="tab"
          class="catalog-tab-panel"
          :role="tab === 'all' ? undefined : 'tabpanel'"
          :aria-labelledby="tab === 'all' ? undefined : `${panelId}-${tab}`"
        >
          <template v-if="tab === 'songs' || tab === 'all'">
            <div class="catalog-section-heading" v-if="artist">
              <h1 v-if="tab === 'all'" class="artist-all-title">
                全部歌曲 <small>{{ artist?.name }}</small>
              </h1>
              <h2 v-else>热门歌曲 TOP 50</h2>
              <button v-if="tab === 'songs'" class="text-button" @click="choose('all')">
                查看全部歌曲 →</button
              ><button v-else class="text-button" @click="emit('back')">返回热门歌曲</button>
            </div>
            <div class="catalog-actions catalog-track-actions">
              <button
                v-if="tab === 'all'"
                class="primary"
                :disabled="!playable || playBusy || !tracks.some((t) => !t.missing)"
                @click="play(tracks.findIndex((t) => !t.missing))"
              >
                <Icon name="play" />{{ tab === 'all' ? '播放已加载歌曲' : '播放全部' }}
              </button>
              <button
                :disabled="downloadBusy || !tracks.some((t) => !t.missing)"
                @click="
                  emit(
                    'download',
                    tracks.filter((t) => !t.missing),
                  )
                "
              >
                下载当前歌曲列表
              </button>
              <span v-if="tab === 'all'" ref="artistBatchAnchor">
                <ActionMenu
                  label="更多歌手歌曲操作"
                  :items="[
                    { key: 'batch', label: '批量操作', icon: 'list' },
                    {
                      key: 'queue',
                      label: '添加已加载歌曲至播放列表',
                      icon: 'plus',
                      disabled: !queueable || !selections.length,
                    },
                  ]"
                  :disabled="!tracks.some((track) => !track.missing && !track.local)"
                  @select="artistSongsAction"
                />
              </span>
              <span class="muted"
                >{{ tracks.length }} 首<span v-if="tab === 'all' && artist">
                  / {{ artist.songCount }} 首</span
                ></span
              >
            </div>
            <p v-if="artistQueueNotice" class="notice" role="status">{{ artistQueueNotice }}</p>
            <TrackTable
              v-if="tracks.length"
              navigable
              :tracks="tracks"
              :liked="liked"
              :busy="busyLikes"
              :playable="playable && !playBusy"
              :current-id="currentId"
              :current-toggle="currentToggle"
              @play="play"
              @toggle-current="(target, anchor) => emit('toggleCurrent', target, anchor)"
              @like="emit('like', $event)"
              @more="emit('more', $event)"
              @artist="emit('open', { kind: 'artist', id: $event })"
              @album="emit('open', { kind: 'album', id: $event })"
            />
            <p v-else-if="!list.loading.value && !list.error.value" class="empty-state">暂无歌曲</p>
          </template>
          <template v-else-if="tab === 'intro'">
            <article v-if="intro" class="catalog-description">
              <p>{{ intro.briefDescription }}</p>
              <section v-for="(section, i) in intro.sections" :key="i">
                <h2>{{ section.title }}</h2>
                <p>{{ section.text }}</p>
              </section>
              <p v-if="!intro.briefDescription && !intro.sections.length">暂无歌手介绍</p>
            </article>
          </template>
          <template v-else-if="tab === 'videos'">
            <ArtistMvGrid
              v-if="artistMvAvailable"
              :videos="artistVideos"
              :continuation="next?.kind === 'artist_videos' ? next.cursor : null"
              :loading="list.loading.value || list.loadingMore.value"
              :error="list.error.value"
              @play="emit('mv', $event)"
              @comments="(id, title) => emit('mvComments', id, title)"
              @more="list.more"
              @retry="list.retry"
            />
            <p v-else class="empty-state">暂无 MV</p>
          </template>
          <CatalogCards
            v-else-if="list.pages.value.length"
            v-bind="cards"
            :playable="playable && !playBusy"
            :queueable="queueable"
            :origin="origin"
            :playing="playing"
            @open="emit('open', $event)"
            @play-album="emit('playAlbum', $event)"
            @album-action="emit('albumAction', $event)"
            @play-artist="emit('playArtist', $event)"
            @artist-action="emit('artistAction', $event)"
          />
          <p v-if="tab !== 'videos' && list.loading.value" class="catalog-loading" role="status">
            正在加载…
          </p>
          <button
            v-if="tab !== 'videos' && next"
            class="load-more"
            :disabled="list.loading.value"
            @click="list.more"
          >
            {{ list.loadingMore.value ? '正在加载…' : '加载更多' }}
          </button>
        </div>
      </BoundedPanelTransition>
    </template>
    <ModalTransition :return-focus="() => batchOrigin" @left="finishTrackBatch">
      <CatalogTrackBatchDialog
        v-if="batchTracks"
        :tracks="batchTracks"
        :scope="resource.kind"
        :liked="liked"
        :current-id="currentId"
        :playable="playable && !playBusy"
        :queueable="queueable && (!!batchAlbumRevision || !!batchArtistPages)"
        @close="batchTracks = undefined"
        @choose="chooseTrackBatch"
      />
    </ModalTransition>
  </section>
</template>
