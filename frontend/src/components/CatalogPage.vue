<script setup lang="ts">
import { computed, onUnmounted, ref, watch } from 'vue';
import type { AccountView, TrackView } from '../services/contracts';
import type { CatalogResource, CatalogQuery, CatalogSelectionPage } from '../services/catalog';
import { nextCatalogQuery } from '../services/catalogPages';
import { useCatalog } from '../composables/useCatalog';
import { useCatalogPages } from '../composables/useCatalogPages';
import { useCatalogMembership } from '../composables/useCatalogMembership';
import TrackTable from './TrackTable.vue';
import CatalogCards from './catalog/CatalogCards.vue';
import Icon from './Icon.vue';

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
  more: [track: TrackView];
  download: [tracks: TrackView[]];
  mv: [id: string];
  comments: [id: string, title: string];
  expired: [];
}>();
const expire = () => emit('expired');
const detail = useCatalog(undefined, expire),
  dynamic = useCatalog(undefined, expire),
  list = useCatalogPages(undefined, expire),
  membership = useCatalogMembership(undefined, expire);
type Tab = 'songs' | 'all' | 'albums' | 'videos' | 'intro' | 'similar' | 'concerts';
const tab = ref<Tab>('songs');
const tabs: { key: Tab; label: string }[] = [
  { key: 'songs', label: '歌曲' },
  { key: 'albums', label: '专辑' },
  { key: 'videos', label: '视频' },
  { key: 'intro', label: '歌手介绍' },
  { key: 'similar', label: '相似歌手' },
  { key: 'concerts', label: '演出' },
];
const artist = computed(() =>
  detail.view.value?.page.kind === 'artist' ? detail.view.value.page.artist : undefined,
);
const album = computed(() =>
  detail.view.value?.page.kind === 'album' ? detail.view.value.page.album : undefined,
);
const name = computed(
  () =>
    artist.value?.name ?? album.value?.name ?? (props.resource.kind === 'artist' ? '歌手' : '专辑'),
);
const cover = computed(() => artist.value?.coverUrl ?? album.value?.coverUrl);
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
  tab.value = next;
  const query = queryFor(next);
  if (query) await list.open(query, refresh);
  else list.clear();
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
function date(value: number | null | undefined) {
  return value == null ? '' : new Date(value).toLocaleDateString();
}
watch(
  () => [props.account.epoch, props.account.status, props.resource.kind, props.resource.id],
  () => {
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
  detail.dispose();
  dynamic.dispose();
  list.dispose();
  membership.dispose();
});
</script>
<template>
  <section class="catalog-page" :aria-label="`${resource.kind === 'artist' ? '歌手' : '专辑'}详情`">
    <div v-if="detail.error.value" class="catalog-error" role="alert">
      {{ detail.error.value }}<span v-if="detail.view.value"> · 保留上次内容</span
      ><button class="text-button" :disabled="detail.loading.value" @click="refresh">重试</button>
    </div>
    <div v-if="!detail.view.value" class="empty-state" role="status">
      <span v-if="detail.loading.value" class="spinner" />{{
        detail.loading.value ? '正在加载详情…' : '暂无可显示的详情'
      }}
    </div>
    <template v-else>
      <header class="catalog-header">
        <img
          v-if="cover"
          class="catalog-cover"
          :class="{ artist: !!artist }"
          :src="cover"
          alt=""
          referrerpolicy="no-referrer"
          decoding="async"
        />
        <div v-else class="catalog-cover catalog-placeholder">
          <Icon :name="artist ? 'user' : 'music'" />
        </div>
        <div class="catalog-heading">
          <span class="catalog-label">{{ artist ? '歌手' : '专辑' }}</span>
          <h1>{{ name }}</h1>
          <p v-if="artist?.aliases.length" class="muted">{{ artist.aliases.join(' / ') }}</p>
          <p v-if="artist" class="muted">
            歌曲 {{ artist.songCount }} · 专辑 {{ artist.albumCount
            }}<span v-if="artist.mvCount != null"> · MV {{ artist.mvCount }}</span>
          </p>
          <template v-if="album">
            <p class="catalog-credits">
              歌手：<template v-for="(credit, i) in album.artists" :key="i"
                ><span v-if="i"> / </span
                ><button
                  v-if="credit.id"
                  class="text-button"
                  @click="emit('open', { kind: 'artist', id: credit.id })"
                >
                  {{ credit.name }}</button
                ><span v-else>{{ credit.name }}</span></template
              >
            </p>
            <p v-if="album.translations.length" class="muted">
              {{ album.translations.join(' / ') }}
            </p>
            <p class="muted">
              {{ album.trackCount }} 首歌曲<span v-if="album.publishedAt != null">
                · 发行于 {{ date(album.publishedAt) }}</span
              >
            </p>
          </template>
          <div class="catalog-actions">
            <button
              :disabled="
                subscribed == null ||
                !membership.known.value ||
                membership.busy.value ||
                membership.pending.value
              "
              :aria-pressed="subscribed ?? undefined"
              @click="collect()"
            >
              <Icon name="star" />{{
                subscribed == null ? '收藏状态未知' : subscribed ? '已收藏' : '收藏'
              }}
            </button>
            <button v-if="album" @click="emit('comments', album.id, album.name)">
              <Icon name="comment" />专辑评论
            </button>
            <button
              class="icon-button"
              aria-label="刷新详情"
              :disabled="detail.loading.value || list.loading.value || membership.busy.value"
              @click="refresh"
            >
              <Icon name="refresh" />
            </button>
          </div>
        </div>
      </header>
      <p v-if="detail.loading.value" class="muted" role="status">正在更新详情…</p>
      <div v-if="dynamic.error.value || membership.error.value" class="catalog-error" role="alert">
        {{ dynamic.error.value || membership.error.value
        }}<button class="text-button" :disabled="membership.busy.value" @click="refreshState">
          重读收藏状态
        </button>
      </div>
      <div v-if="membership.pending.value" class="notice" role="status">
        上次收藏操作结果待确认<button
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
      <nav v-if="artist" class="catalog-tabs" aria-label="歌手内容">
        <button
          v-for="item in tabs"
          :key="item.key"
          :aria-current="
            tab === item.key || (item.key === 'songs' && tab === 'all') ? 'page' : undefined
          "
          @click="choose(item.key)"
        >
          {{ item.label }}
        </button>
      </nav>
      <Transition name="catalog-tab" mode="out-in">
        <div :key="tab" class="catalog-tab-panel">
          <template v-if="tab === 'songs' || tab === 'all'">
            <div class="catalog-section-heading" v-if="artist">
              <h2>{{ tab === 'all' ? '全部歌曲' : '热门歌曲 TOP 50' }}</h2>
              <button v-if="tab === 'songs'" class="text-button" @click="choose('all')">
                查看全部歌曲 →</button
              ><button v-else class="text-button" @click="choose('songs')">返回热门歌曲</button>
            </div>
            <div class="catalog-actions catalog-track-actions">
              <button
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
            <details v-if="album?.description" class="catalog-description">
              <summary>专辑介绍</summary>
              <p>{{ album.description }}</p>
            </details>
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
          <div v-if="list.error.value" class="catalog-error" role="alert">
            {{ list.error.value
            }}<button
              class="text-button"
              :disabled="list.loading.value"
              @click="next ? list.more() : choose(tab, true)"
            >
              重试
            </button>
          </div>
          <button v-if="next" class="load-more" :disabled="list.loading.value" @click="list.more">
            {{ list.loadingMore.value ? '正在加载…' : '加载更多' }}
          </button>
        </div>
      </Transition>
    </template>
  </section>
</template>
