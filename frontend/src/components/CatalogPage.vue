<script setup lang="ts">
import type { TrackMenuRequest } from '../services/trackMenu';
import { computed, onUnmounted, ref, useId, watch } from 'vue';
import type { AccountView, TrackView } from '../services/contracts';
import type {
  ArtistTab,
  CatalogResource,
  CatalogQuery,
  CatalogSelectionPage,
} from '../services/catalog';
import { nextCatalogQuery } from '../services/catalogPages';
import { useCatalog } from '../composables/useCatalog';
import { useArtistPages } from '../composables/useArtistPages';
import { useCatalogMembership } from '../composables/useCatalogMembership';
import TrackTable from './TrackTable.vue';
import CatalogCards from './catalog/CatalogCards.vue';
import AlbumHeader from './catalog/AlbumHeader.vue';
import AlbumContent from './catalog/AlbumContent.vue';
import Icon from './Icon.vue';
import ArtistHeader from './catalog/ArtistHeader.vue';
import ArtistTabs from './catalog/ArtistTabs.vue';

const props = defineProps<{
  resource: CatalogResource;
  account: AccountView;
  playable: boolean;
  playBusy: boolean;
  downloadBusy: boolean;
  liked: Set<string>;
  busyLikes: Set<string>;
  currentId?: string;
}>();
const emit = defineEmits<{
  open: [resource: CatalogResource];
  play: [pages: CatalogSelectionPage[], index: number];
  like: [track: TrackView];
  more: [request: TrackMenuRequest];
  download: [tracks: TrackView[]];
  mv: [id: string];
  expired: [];
}>();
const expire = () => emit('expired');
const detail = useCatalog(undefined, expire),
  dynamic = useCatalog(undefined, expire),
  membership = useCatalogMembership(undefined, expire);
type Tab = ArtistTab;
const tab = ref<Tab>('songs');
const list = useArtistPages(tab, undefined, expire);
const section = ref<HTMLElement>(),
  panel = ref<HTMLElement>();
const panelId = useId();
const bookmarks = new Map<Tab, { inner: number; outer: number }>();
let pendingRestore: Tab | undefined,
  restoring = 0;
function remember() {
  if (panel.value?.dataset.artistPanel !== tab.value) return;
  const inner = panel.value?.querySelector<HTMLElement>('.track-scroll,.catalog-card-scroll');
  bookmarks.set(tab.value, { inner: inner?.scrollTop ?? 0, outer: section.value?.scrollTop ?? 0 });
}
function cancelRestore(event?: Event) {
  if (event instanceof KeyboardEvent && (event.target as Element)?.closest('.catalog-tabs')) return;
  pendingRestore = undefined;
  restoring++;
}
function restore(element: Element) {
  const owner = tab.value,
    saved = bookmarks.get(owner),
    ticket = ++restoring;
  if (pendingRestore !== owner || !saved) return;
  requestAnimationFrame(() => {
    // Let the newly inserted grid measure its columns and commit its spacers
    // before restoring. Use the entering node, not a transition's changing ref.
    requestAnimationFrame(() => {
      if (
        ticket !== restoring ||
        pendingRestore !== owner ||
        tab.value !== owner ||
        !element.isConnected ||
        (element as HTMLElement).dataset.artistPanel !== owner
      )
        return;
      const inner = element.querySelector<HTMLElement>('.track-scroll,.catalog-card-scroll');
      if (inner) {
        inner.scrollTop = saved.inner;
        inner.dispatchEvent(new Event('scroll'));
      }
      if (section.value) section.value.scrollTop = saved.outer;
      pendingRestore = undefined;
    });
  });
}
const artist = computed(() =>
  detail.view.value?.page.kind === 'artist' ? detail.view.value.page.artist : undefined,
);
const album = computed(() =>
  detail.view.value?.page.kind === 'album' ? detail.view.value.page.album : undefined,
);
const hotTracks = computed(() =>
  detail.view.value?.page.kind === 'artist' ? detail.view.value.page.tracks : [],
);
const onlineCount = computed(() =>
  dynamic.view.value?.page.kind === 'artist_dynamic'
    ? dynamic.view.value.page.onlineCount
    : undefined,
);
function playHot() {
  const index = hotTracks.value.findIndex((track) => !track.missing);
  if (index >= 0 && detail.view.value)
    emit('play', [{ query: detail.view.value.query, revision: detail.view.value.revision }], index);
}
const observed = ref<boolean | null>(null);
const subscribed = computed(
  () =>
    observed.value ??
    (props.resource.kind === 'album'
      ? album.value?.subscribed
      : dynamic.view.value?.page.kind === 'artist_dynamic'
        ? dynamic.view.value.page.followed
        : null),
);
const tracks = computed(() => {
  const p = detail.view.value?.page;
  if (tab.value === 'songs') return p?.kind === 'artist' || p?.kind === 'album' ? p.tracks : [];
  return tab.value === 'all'
    ? list.pages.value.flatMap((v) => (v.page.kind === 'artist_songs' ? v.page.tracks : []))
    : [];
});
const selections = computed<CatalogSelectionPage[]>(() =>
  (tab.value === 'songs' ? (detail.view.value ? [detail.view.value] : []) : list.pages.value).map(
    (v) => ({ query: v.query, revision: v.revision }),
  ),
);
const next = computed(() => {
  const last = list.pages.value.at(-1);
  return last ? nextCatalogQuery(last) : null;
});
const intro = computed(() =>
  list.pages.value[0]?.page.kind === 'artist_introduction' ? list.pages.value[0].page : undefined,
);
const auxiliary = computed(() =>
  detail.view.value?.page.kind === 'album' ? detail.view.value.page.read : undefined,
);
const cards = computed(() => ({
  albums: list.pages.value.flatMap((v) => (v.page.kind === 'artist_albums' ? v.page.albums : [])),
  artists: list.pages.value.flatMap((v) =>
    v.page.kind === 'similar_artists' ? v.page.artists : [],
  ),
  videos: list.pages.value.flatMap((v) => (v.page.kind === 'artist_videos' ? v.page.videos : [])),
  concerts: list.pages.value.flatMap((v) =>
    v.page.kind === 'artist_concerts' ? v.page.concerts : [],
  ),
}));
function queryFor(next: Tab): CatalogQuery | undefined {
  const id = props.resource.id;
  switch (next) {
    case 'all':
      return { kind: 'artist_songs', id, offset: 0, limit: 20 };
    case 'albums':
      return { kind: 'artist_albums', id, offset: 0, limit: 20 };
    case 'videos':
      return { kind: 'artist_videos', id, cursor: '0', size: 20 };
    case 'intro':
      return { kind: 'artist_introduction', id };
    case 'similar':
      return { kind: 'similar_artists', id, offset: 0, limit: 20 };
    case 'concerts':
      return { kind: 'artist_concerts', id, offset: 0, limit: 20 };
  }
}
async function choose(next: Tab, refresh = false) {
  if (next !== tab.value) {
    if (pendingRestore !== tab.value) remember();
    restoring++;
    pendingRestore = bookmarks.has(next) ? next : undefined;
  }
  tab.value = next;
  const query = queryFor(next);
  if (query) await list.open(query, refresh);
}
async function refreshState() {
  await Promise.all([
    membership.command({ kind: 'inspect' }),
    props.resource.kind === 'artist'
      ? dynamic.open({ kind: 'artist_dynamic', id: props.resource.id }, true)
      : Promise.resolve(),
  ]);
}
async function refresh() {
  observed.value = null;
  await Promise.all([
    detail.open(props.resource, true),
    refreshState(),
    tab.value === 'songs' ? Promise.resolve() : choose(tab.value, true),
  ]);
}
async function collect(reconcile = false) {
  if (!reconcile && subscribed.value == null) return;
  const value = await membership.command(
    reconcile ? { kind: 'reconcile' } : { kind: 'set', desired: !subscribed.value },
  );
  if (value !== null) observed.value = value;
}
function play(index: number) {
  if (tracks.value[index] && !tracks.value[index].missing) emit('play', selections.value, index);
}
watch(
  [
    () => props.account.epoch,
    () => props.account.status,
    () => props.resource.kind,
    () => props.resource.id,
  ],
  () => {
    cancelRestore();
    bookmarks.clear();
    tab.value = 'songs';
    observed.value = null;
    list.clear();
    detail.connect(props.account);
    dynamic.connect(props.account);
    list.connect(props.account);
    membership.connect(props.account, props.resource);
    void detail.open(props.resource);
    if (props.resource.kind === 'artist')
      void dynamic.open({ kind: 'artist_dynamic', id: props.resource.id });
    void membership.command({ kind: 'inspect' });
  },
  { immediate: true },
);
onUnmounted(() => {
  cancelRestore();
  detail.dispose();
  dynamic.dispose();
  list.dispose();
  membership.dispose();
});
function back() {
  if (!artist.value || tab.value !== 'all') return false;
  void choose('songs');
  return true;
}
defineExpose({
  back,
  canGoBack: computed(() => !!artist.value && tab.value === 'all'),
  resourceKey: computed(() => `${props.resource.kind}:${props.resource.id}`),
});
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
      v-if="detail.error.value || list.error.value"
      class="catalog-error catalog-detail-error"
      role="alert"
    >
      <div v-if="detail.error.value">
        {{ detail.error.value }}<span v-if="detail.view.value"> · 保留上次内容</span
        ><button class="text-button" :disabled="detail.loading.value" @click="refresh">重试</button>
      </div>
      <div v-if="list.error.value" class="catalog-list-error">
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
      <AlbumHeader
        v-if="album"
        :album="album"
        :digital="auxiliary?.digitalAlbumTag === true"
        :subscribed="subscribed ?? null"
        :collect-disabled="
          subscribed == null ||
          !membership.known.value ||
          membership.busy.value ||
          membership.pending.value
        "
        :play-disabled="!playable || playBusy || !tracks.some((t) => !t.missing)"
        :download-disabled="downloadBusy || !tracks.some((t) => !t.missing)"
        :refresh-disabled="detail.loading.value || membership.busy.value"
        @open="emit('open', $event)"
        @play="play(tracks.findIndex((t) => !t.missing))"
        @collect="collect()"
        @download="
          emit(
            'download',
            tracks.filter((t) => !t.missing),
          )
        "
        @refresh="refresh"
      />
      <ArtistHeader
        v-else-if="artist && tab !== 'all'"
        :artist="artist"
        :followed="subscribed ?? null"
        :follow-disabled="
          subscribed == null ||
          !membership.known.value ||
          membership.busy.value ||
          membership.pending.value
        "
        :play-disabled="!playable || playBusy || !hotTracks.some((track) => !track.missing)"
        :refresh-disabled="detail.loading.value || list.loading.value || membership.busy.value"
        @play="playHot"
        @follow="collect()"
        @refresh="refresh"
      />
      <p v-if="detail.loading.value" class="muted" role="status">正在更新详情…</p>
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
        :account="account"
        :tracks="tracks"
        :liked="liked"
        :busy-likes="busyLikes"
        :playable="playable && !playBusy"
        :current-id="currentId"
        @play="play"
        @like="emit('like', $event)"
        @more="emit('more', $event)"
        @open="emit('open', $event)"
        @expired="expire"
      />
      <Transition
        v-else
        name="catalog-tab"
        mode="out-in"
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
              ><button v-else class="text-button" @click="choose('songs')">返回热门歌曲</button>
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
              <span class="muted"
                >{{ tracks.length }} 首<span v-if="tab === 'all' && artist">
                  / {{ artist.songCount }} 首</span
                ></span
              >
            </div>
            <TrackTable
              v-if="tracks.length"
              navigable
              :tracks="tracks"
              :liked="liked"
              :busy="busyLikes"
              :playable="playable && !playBusy"
              :current-id="currentId"
              @play="play"
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
          <CatalogCards
            v-else-if="list.pages.value.length"
            v-bind="cards"
            @open="emit('open', $event)"
            @mv="emit('mv', $event)"
          />
          <p v-if="list.loading.value" class="catalog-loading" role="status">正在加载…</p>
          <button v-if="next" class="load-more" :disabled="list.loading.value" @click="list.more">
            {{ list.loadingMore.value ? '正在加载…' : '加载更多' }}
          </button>
        </div>
      </Transition>
    </template>
  </section>
</template>
