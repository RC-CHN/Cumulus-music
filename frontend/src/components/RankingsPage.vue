<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import type { useRankings } from '../composables/useRankings';
import type { RankingCard, RankingQuery } from '../services/rankings';
import type { TrackView } from '../services/contracts';
import RankingCards from './RankingCards.vue';
import TrackTable from './TrackTable.vue';
import Icon from './Icon.vue';
const props = defineProps<{
  model: ReturnType<typeof useRankings>;
  playable: boolean;
  liked: Set<string>;
  busyLikes: Set<string>;
  currentId?: string;
}>();
const emit = defineEmits<{
  navigate: [query: RankingQuery];
  playlist: [id: string];
  playPlaylist: [id: string];
  play: [tracks: TrackView[], index: number];
  playAlbum: [id: string, revision: string, startId: string | null];
  like: [track: TrackView];
  comments: [track: TrackView];
  download: [tracks: TrackView[]];
}>();
const page = computed(() => props.model.view.value?.page);
const period = ref('daily');
const singles = ref(false);
const year = ref(new Date().getFullYear());
const years = Array.from(
  { length: Math.max(1, new Date().getFullYear() - 2018 + 1) },
  (_, i) => 2018 + i,
);
watch(
  () => props.model.query.value,
  (q) => {
    if (q.kind === 'album_year') {
      period.value = 'year';
      year.value = q.year;
    } else if (q.kind === 'album_sales') year.value = q.year;
  },
  { immediate: true },
);
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
function choosePeriod(next: string) {
  period.value = next;
  if (props.model.query.value.kind === 'album_year' && next !== 'year')
    emit('navigate', { kind: 'album_sales', year: new Date().getFullYear() });
}
function chooseYear() {
  period.value = 'year';
  emit('navigate', { kind: 'album_year', year: year.value });
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
  if (p?.kind === 'album') emit('playAlbum', p.id, p.revision, p.tracks[index]?.id ?? null);
  else emit('play', tracks(), index);
}
function change(n: number | null, newest: boolean) {
  return newest ? '新' : n == null ? '' : n === 0 ? '—' : n > 0 ? `↑${n}` : `↓${-n}`;
}
</script>
<template>
  <section class="rankings-page" :aria-busy="model.loading.value">
    <header class="rankings-header">
      <h1>{{ title }}</h1>
      <button
        class="icon-button"
        aria-label="刷新榜单"
        :disabled="model.loading.value"
        @click="model.refresh"
      >
        <Icon name="refresh" />
      </button>
    </header>
    <div v-if="model.error.value" class="rank-error" role="alert">
      {{ model.error.value }}<span v-if="page"> · 保留上次加载的内容</span
      ><button class="text-button" :disabled="model.loading.value" @click="model.refresh">
        重试
      </button>
    </div>
    <p v-if="model.loading.value" role="status">正在加载…</p>
    <RankingCards
      v-if="page?.kind === 'home'"
      :groups="page.groups"
      :playable="playable"
      @open="open"
      @play="(id) => emit('playPlaylist', id)"
    />
    <template v-else-if="page?.kind === 'album_sales'">
      <p class="rank-description">统计网易云音乐数字专辑与数字单曲销量，每 30 分钟更新。</p>
      <div class="rank-filters" aria-label="榜单周期">
        <button
          v-for="item in [
            { id: 'daily', name: '日榜' },
            { id: 'week', name: '周榜' },
            { id: 'total', name: '总榜' },
          ]"
          :key="item.id"
          :aria-pressed="period === item.id"
          @click="choosePeriod(item.id)"
        >
          {{ item.name }}
        </button>
        <button :aria-pressed="period === 'year'" @click="choosePeriod('year')">年榜</button>
        <select v-model.number="year" aria-label="榜单年份" @change="chooseYear">
          <option v-for="y in years" :key="y" :value="y">{{ y }} 年</option>
        </select>
      </div>
      <div class="rank-filters" aria-label="商品类型">
        <button :aria-pressed="!singles" @click="singles = false">数字专辑</button
        ><button :aria-pressed="singles" @click="singles = true">数字单曲</button>
      </div>
      <div class="rank-sales-list">
        <div
          v-for="(album, index) in board?.products ?? []"
          :key="`${index}:${album.id}`"
          class="rank-sale-row"
        >
          <span class="sale-position"
            >{{ index + 1 }}<small>{{ change(album.rankIncr, album.newest) }}</small></span
          >
          <button
            class="sale-album"
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
        >
          <span class="sale-position"
            >{{ index + 1 }}<small>{{ change(row.rankIncr, row.newest) }}</small></span
          >
          <button
            class="sale-album"
            :aria-label="`播放 ${row.track.name}`"
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
        @more="(t) => emit('comments', t)"
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
