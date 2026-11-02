<script setup lang="ts">
import type { TrackMenuRequest } from '../services/trackMenu';
import { computed, ref, nextTick } from 'vue';
import type { useRankings } from '../composables/useRankings';
import { useRankingViewport } from '../composables/useRankingViewport';
import { useRankingPlaybackFocus } from '../composables/useRankingPlaybackFocus';
import type {
  RankingCard,
  RankingPeriod,
  RankingQuery,
  RankingUiState,
} from '../services/rankings';
import type { TrackView } from '../services/contracts';
import type { PlaybackStatus } from '../services/player';
import RankingCards from './RankingCards.vue';
import RankingPeriods from './RankingPeriods.vue';
import TrackTable from './TrackTable.vue';
import Icon from './Icon.vue';
const props = defineProps<{
  model: ReturnType<typeof useRankings>;
  playable: boolean;
  liked: Set<string>;
  busyLikes: Set<string>;
  currentId?: string;
  currentPlaylistId?: string;
  playbackState?: PlaybackStatus;
}>();
const emit = defineEmits<{
  navigate: [query: RankingQuery, state?: RankingUiState];
  playlist: [id: string];
  playPlaylist: [id: string];
  play: [tracks: TrackView[], index: number];
  playAlbum: [id: string, revision: string, startId: string | null];
  like: [track: TrackView];
  more: [request: TrackMenuRequest];
  comments: [track: TrackView];
  download: [tracks: TrackView[]];
}>();
const page = computed(() => props.model.view.value?.page);
const pageRoot = ref<HTMLElement>();
const viewport = useRankingViewport(props.model, pageRoot);
const ownsVisit = props.model.captureVisit();
const playbackFocus = useRankingPlaybackFocus(() => props.playable, pageRoot, ownsVisit);
defineExpose({ checkpoint: viewport.checkpoint });
async function readPage() {
  const origin = document.activeElement;
  await props.model.refresh();
  await nextTick();
  if (ownsVisit() && pageRoot.value?.isConnected && document.activeElement === document.body) {
    const target =
      origin instanceof HTMLElement &&
      pageRoot.value.contains(origin) &&
      !origin.matches(':disabled')
        ? origin
        : pageRoot.value.querySelector<HTMLElement>('[aria-label="刷新榜单"]');
    target?.focus({ preventScroll: true });
    target?.scrollIntoView({ block: 'nearest' });
  }
}
const period = computed(() => props.model.selection.value.period);
const singles = computed({
  get: () => props.model.selection.value.singles,
  set: (value: boolean) => (props.model.selection.value.singles = value),
});
const year = computed(() => {
  const q = props.model.query.value;
  return q.kind === 'album_year' || q.kind === 'album_sales' ? q.year : new Date().getFullYear();
});
const board = computed(() =>
  page.value?.kind === 'album_sales'
    ? page.value.boards.find(
        (b) =>
          b.period === period.value &&
          b.singles === singles.value &&
          (period.value !== 'year' || b.year === year.value),
      )
    : undefined,
);
const title = computed(() =>
  page.value?.kind === 'album'
    ? page.value.name
    : {
        home: '排行榜',
        album_sales: '数字专辑畅销榜',
        album_year: '数字专辑畅销榜',
        song_sales: '歌曲畅销指数榜',
        album: '专辑',
      }[props.model.query.value.kind],
);
function open(card: RankingCard) {
  if (card.target === 'playlist') emit('playlist', card.id);
  else
    emit(
      'navigate',
      card.target === 'album_sales'
        ? { kind: 'album_sales', year: new Date().getFullYear() }
        : { kind: 'song_sales' },
    );
}
function choosePeriod(next: RankingPeriod) {
  if (props.model.query.value.kind === 'album_year' && next !== 'year') {
    const query: RankingQuery = { kind: 'album_sales', year: new Date().getFullYear() };
    emit(
      'navigate',
      query,
      props.model.destination(query, { period: next, singles: singles.value }),
    );
  } else props.model.selection.value.period = next;
}
function chooseYear(value: number) {
  if (year.value === value) choosePeriod('year');
  else {
    const query: RankingQuery = { kind: 'album_year', year: value };
    emit(
      'navigate',
      query,
      props.model.destination(query, { period: 'year', singles: singles.value }),
    );
  }
}
function tracks(): TrackView[] {
  const p = page.value;
  return p?.kind === 'song_sales'
    ? p.songs.map((s) => s.track)
    : p?.kind === 'album'
      ? p.tracks
      : [];
}
function play(index = 0) {
  const p = page.value;
  playbackFocus.start(() => {
    if (p?.kind === 'album') emit('playAlbum', p.id, p.revision, p.tracks[index]?.id ?? null);
    else emit('play', tracks(), index);
  });
}
function playPlaylist(id: string) {
  playbackFocus.start(() => emit('playPlaylist', id));
}
function change(n: number | null, newest: boolean) {
  return newest ? '新' : n == null ? '' : n === 0 ? '—' : n > 0 ? `↑${n}` : `↓${-n}`;
}
</script>
<template>
  <section
    ref="pageRoot"
    class="rankings-page"
    :aria-busy="model.loading.value"
    @focusin="viewport.remember($event.target)"
  >
    <header class="rankings-header">
      <h1 tabindex="-1">{{ title }}</h1>
      <button
        class="icon-button"
        aria-label="刷新榜单"
        :disabled="model.loading.value"
        @click="readPage"
      >
        <Icon name="refresh" />
      </button>
    </header>
    <div v-if="model.error.value && page" class="rank-error" role="alert" data-ranking-read-error>
      {{ model.error.value }}<span v-if="page"> · 保留上次加载的内容</span
      ><button class="text-button" :disabled="model.loading.value" @click="readPage">重试</button>
    </div>
    <div v-if="model.loading.value && !page" class="empty-state" role="status">
      <span class="spinner" />正在读取榜单…
    </div>
    <p v-else-if="model.loading.value" role="status">正在更新榜单…</p>
    <div
      v-else-if="model.error.value && !page"
      class="error-state"
      role="alert"
      data-ranking-read-error
    >
      <h2>榜单暂时无法读取</h2>
      <p>{{ model.error.value }}</p>
      <button class="primary" @click="readPage">重试读取</button>
    </div>
    <RankingCards
      v-if="page?.kind === 'home'"
      :groups="page.groups"
      :playable="playable"
      :current-playlist-id="currentPlaylistId"
      :playback-state="playbackState"
      @open="open"
      @play="playPlaylist"
    />
    <template v-else-if="page?.kind === 'album_sales'">
      <p class="rank-description">统计网易云音乐数字专辑与数字单曲销量，每 30 分钟更新。</p>
      <RankingPeriods :period="period" :year="year" @period="choosePeriod" @year="chooseYear" />
      <div class="rank-filters" aria-label="商品类型">
        <button :aria-pressed="!singles" @click="singles = false">数字专辑</button
        ><button :aria-pressed="singles" @click="singles = true">数字单曲</button>
      </div>
      <div class="rank-sales-list">
        <div
          v-for="(album, index) in board?.products ?? []"
          :key="`${index}:${album.id}`"
          class="rank-sale-row"
          :data-sale-id="album.id"
        >
          <span class="sale-position"
            >{{ index + 1 }}<small>{{ change(album.rankIncr, album.newest) }}</small></span
          >
          <button
            class="sale-album"
            data-rank-action="open"
            :aria-label="`查看专辑 ${album.name}`"
            @click="emit('navigate', { kind: 'album', id: album.id })"
          >
            <img
              v-if="album.coverUrl"
              :src="album.coverUrl"
              alt=""
              loading="lazy"
              referrerpolicy="no-referrer"
            /><span
              ><strong>{{ album.name }}</strong
              ><small>{{ album.artist }}</small></span
            >
          </button>
          <span class="sale-number"
            >{{ album.saleNum == null ? '—' : album.saleNum.toLocaleString()
            }}<small>销量</small></span
          >
        </div>
        <p v-if="!board?.products.length" class="empty-state">该周期暂无榜单数据</p>
      </div>
    </template>
    <template v-else-if="page?.kind === 'song_sales'">
      <p class="rank-description">
        在售单曲当日销量前 100 名，按畅销指数排序。每小时更新，每日零点重置。<span
          v-if="page.generatedAt"
        >
          更新于 {{ new Date(page.generatedAt).toLocaleString() }}</span
        >
      </p>
      <div class="rank-actions">
        <button
          class="primary"
          :disabled="!playable || !tracks().some((t) => !t.missing)"
          @click="play(tracks().findIndex((t) => !t.missing))"
        >
          <Icon name="play" />播放全部</button
        ><button :disabled="!tracks().length" @click="emit('download', tracks())">下载</button>
      </div>
      <div class="rank-sales-list song-sales-list">
        <div
          v-for="(row, index) in page.songs"
          :key="`${index}:${row.track.id}`"
          class="rank-sale-row"
          :class="{ current: currentId === row.track.id }"
          :data-sale-song-id="row.track.id"
        >
          <span class="sale-position"
            >{{ index + 1 }}<small>{{ change(row.rankIncr, row.newest) }}</small></span
          >
          <button
            class="sale-album"
            :aria-label="`播放 ${row.track.name}`"
            data-rank-action="play"
            :disabled="!playable || row.track.missing"
            @click="play(index)"
          >
            <img
              v-if="row.track.coverUrl"
              :src="row.track.coverUrl"
              alt=""
              loading="lazy"
              referrerpolicy="no-referrer"
            /><span
              ><strong>{{ row.track.name }}</strong
              ><small>{{ row.track.artists.join(' / ') }}</small></span
            >
          </button>
          <span class="sale-number"
            >{{ row.indicator?.toFixed(2) ?? '—' }}<small>畅销指数</small></span
          >
          <button
            class="icon-button like-button"
            data-rank-action="like"
            :class="{ liked: liked.has(row.track.id) }"
            :aria-pressed="liked.has(row.track.id)"
            :aria-label="`${liked.has(row.track.id) ? '取消喜欢' : '喜欢'} ${row.track.name}`"
            :disabled="row.track.missing || busyLikes.has(row.track.id)"
            @click="emit('like', row.track)"
          >
            <Icon name="heart" />
          </button>
          <button
            class="icon-button"
            :aria-label="`评论 ${row.track.name}`"
            data-rank-action="comments"
            @click="emit('comments', row.track)"
          >
            <Icon name="comment" />
          </button>
        </div>
        <p v-if="!page.songs.length" class="empty-state">暂无上榜歌曲</p>
      </div>
    </template>
    <template v-else-if="page?.kind === 'album'">
      <div class="rank-album-info">
        <img
          v-if="page.coverUrl"
          :src="page.coverUrl"
          alt="专辑封面"
          referrerpolicy="no-referrer"
        />
        <div>
          <p>{{ page.artists.join(' / ') }}</p>
          <p>{{ page.tracks.length }} 首歌曲</p>
          <details v-if="page.description">
            <summary>专辑介绍</summary>
            <p>{{ page.description }}</p>
          </details>
        </div>
      </div>
      <div class="rank-actions">
        <button class="primary" :disabled="!playable || !page.tracks.length" @click="play()">
          <Icon name="play" />播放全部</button
        ><button :disabled="!page.tracks.length" @click="emit('download', page.tracks)">
          下载
        </button>
      </div>
      <TrackTable
        :tracks="page.tracks"
        :liked="liked"
        :busy="busyLikes"
        :playable="playable"
        :current-id="currentId"
        @play="play"
        @like="(t) => emit('like', t)"
        @more="emit('more', $event)"
      />
    </template>
  </section>
</template>
<style scoped>
.rankings-page {
  min-width: 0;
  padding-bottom: 24px;
}
.rankings-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
}
.rankings-header h1 {
  font-size: 26px;
}
.rank-description {
  color: var(--text-secondary);
  line-height: 1.7;
  font-size: 13px;
}
.rank-error {
  color: var(--danger);
  padding: 8px 0;
}
.rank-filters,
.rank-actions {
  display: flex;
  flex-wrap: wrap;
  gap: 10px;
  margin: 16px 0;
}
.rank-filters button,
.rank-filters select,
.rank-actions > button {
  padding: 7px 16px;
  border: 1px solid var(--line);
  border-radius: 20px;
  background: var(--surface);
  color: inherit;
}
.rank-actions .primary {
  background: var(--accent-fill);
  color: var(--on-accent);
  display: flex;
  align-items: center;
  gap: 7px;
}
.rank-filters [aria-pressed='true'] {
  background: var(--surface-alt);
  color: var(--accent);
  border-color: var(--line);
}
.rank-sale-row {
  display: flex;
  align-items: center;
  gap: 14px;
  min-height: 74px;
  padding: 8px 12px;
}
.rank-sale-row:nth-child(even) {
  background: var(--surface);
}
.rank-sale-row.current {
  background: var(--surface-alt);
}
.sale-position {
  width: 40px;
  flex-shrink: 0;
  font-size: 17px;
  text-align: center;
  color: var(--text-secondary);
}
.sale-position small {
  display: block;
  font-size: 10px;
  margin-top: 4px;
}
.sale-album {
  min-width: 0;
  display: flex;
  flex: 1;
  align-items: center;
  gap: 14px;
  text-align: left;
  background: none;
  border: 0;
  color: inherit;
  padding: 0;
}
.sale-album img {
  width: 54px;
  height: 54px;
  object-fit: cover;
  border-radius: 6px;
}
.sale-album > span {
  min-width: 0;
}
.sale-album strong {
  display: block;
  font-size: 14px;
  font-weight: 500;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.sale-album small {
  display: block;
  color: var(--muted);
  margin-top: 5px;
  font-size: 12px;
}
.sale-number {
  min-width: 70px;
  text-align: right;
  font-variant-numeric: tabular-nums;
  color: var(--accent);
}
.sale-number small {
  display: block;
  color: var(--muted);
  font-size: 11px;
  margin-top: 4px;
}
.rank-album-info {
  display: flex;
  gap: 24px;
  align-items: flex-start;
}
.rank-album-info > img {
  width: 140px;
  aspect-ratio: 1;
  border-radius: 10px;
}
.rank-album-info > div {
  min-width: 0;
}
.rank-album-info details p {
  white-space: pre-wrap;
  font-size: 13px;
  line-height: 1.7;
}
.rankings-page :deep(.track-table) {
  height: 480px;
  max-height: 60vh;
}
@media (max-width: 900px) {
  .rank-sale-row {
    gap: 8px;
    padding: 8px 0;
  }
  .sale-album {
    gap: 8px;
  }
  .sale-album img {
    width: 40px;
    height: 40px;
  }
  .sale-position {
    width: 24px;
  }
  .sale-number {
    min-width: 52px;
    font-size: 12px;
  }
  .rank-album-info > img {
    width: 100px;
  }
}
</style>
