<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, watch } from 'vue';
import Icon from '../Icon.vue';
import FollowArtistNoticeMenu from './FollowArtistNoticeMenu.vue';
import { useCore } from '../../services/core';
import { useFollowNewSong } from '../../composables/useFollowNewSong';
import type { TrackView } from '../../services/contracts';
import type { FollowRelease } from '../../services/followNewSong';

const props = defineProps<{ epoch: string; playable: boolean }>();
const emit = defineEmits<{
  expired: [];
  play: [tracks: TrackView[], index: number];
  album: [id: string];
  playAlbum: [id: string];
  artist: [id: string];
  mv: [id: string];
}>();
const model = useFollowNewSong(useCore(), () => emit('expired'));
const {
  releases: loadedReleases,
  latest,
  latestVisitTime,
  cursor,
  pending,
  initial,
  latestPending,
  error,
  latestError,
  group,
  groups,
  autoMore,
} = model;
const surface = ref<HTMLElement>();
const heading = ref<HTMLElement>();
const playAll = ref<HTMLButtonElement>();
const sentinel = ref<HTMLElement>();
const expanded = ref<number | null>(null);
const mutedArtists = ref(new Set<string>());
const notificationNotice = ref('');
const releases = computed(() =>
  loadedReleases.value.filter((release) => !mutedArtists.value.has(release.artistId)),
);
const trackWindow = ref(0);
const dayFormat = new Intl.DateTimeFormat('zh-CN', {
  year: 'numeric',
  month: 'long',
  day: 'numeric',
});
const clockFormat = new Intl.DateTimeFormat('zh-CN', {
  hour: '2-digit',
  minute: '2-digit',
  hour12: false,
});
const TRACK_WINDOW = 24;

watch(
  () => props.epoch,
  (epoch) => {
    mutedArtists.value = new Set();
    notificationNotice.value = '';
    model.connect(epoch);
  },
  { immediate: true },
);
onBeforeUnmount(model.clear);
watch(group, () => {
  expanded.value = null;
  trackWindow.value = 0;
});
watch(
  sentinel,
  (element, _, cleanup) => {
    if (!element) return;
    const observer = new IntersectionObserver(
      ([entry]) => {
        if (entry?.isIntersecting && autoMore.value && !error.value) void model.read();
      },
      { root: surface.value ?? null, rootMargin: '0px 0px 75px 0px' },
    );
    observer.observe(element);
    cleanup(() => observer.disconnect());
  },
  { flush: 'post' },
);

const historyIndex = computed(() =>
  releases.value.findIndex(
    (release, index, rows) =>
      latestVisitTime.value !== null &&
      index > 0 &&
      index < rows.length - 1 &&
      rows[index - 1]!.publishTime > latestVisitTime.value &&
      latestVisitTime.value > release.publishTime,
  ),
);
const date = (time: number) => dayFormat.format(time);
const clock = (time: number) => clockFormat.format(time);

function expand(index: number) {
  expanded.value = expanded.value === index ? null : index;
  trackWindow.value = 0;
}
function displayed(release: FollowRelease, index: number) {
  if (expanded.value !== index) return release.kind === 'album' ? [] : release.tracks.slice(0, 1);
  return release.tracks.slice(
    trackWindow.value * TRACK_WINDOW,
    (trackWindow.value + 1) * TRACK_WINDOW,
  );
}
function playTrack(release: FollowRelease, index: number, releaseIndex: number) {
  const offset = expanded.value === releaseIndex ? trackWindow.value * TRACK_WINDOW : 0;
  if (release.kind === 'song') {
    const item = release.tracks[offset + index];
    if (item) emit('play', [item.song], 0);
    return;
  }
  emit(
    'play',
    release.tracks.map(({ song }) => song),
    offset + index,
  );
}
async function pickGroup(index: number) {
  model.chooseGroup(index);
  await nextTick();
  heading.value?.focus({ preventScroll: true });
  heading.value?.scrollIntoView({ block: 'start', behavior: 'instant' });
}
async function more() {
  const before = group.value;
  const opener = document.activeElement;
  await model.read();
  if (before !== group.value) {
    await pickGroup(group.value);
  } else if (
    opener instanceof HTMLElement &&
    !opener.isConnected &&
    document.activeElement === document.body
  ) {
    heading.value?.focus({ preventScroll: true });
  }
}
async function retryLatest() {
  const opener = document.activeElement;
  await model.latestRead();
  if (
    opener instanceof HTMLElement &&
    !opener.isConnected &&
    document.activeElement === document.body
  ) {
    const target = playAll.value?.disabled ? heading.value : playAll.value;
    target?.focus({ preventScroll: true });
  }
}
async function hideMutedArtist(id: string) {
  mutedArtists.value.add(id);
  expanded.value = null;
  trackWindow.value = 0;
  notificationNotice.value = '该歌手新歌通知已取消';
  await nextTick();
  heading.value?.focus({ preventScroll: true });
}
</script>

<template>
  <section ref="surface" class="follow-release-page" aria-labelledby="follow-release-title">
    <header class="page-heading follow-release-heading">
      <div>
        <h1 id="follow-release-title" ref="heading" tabindex="-1">新歌发布</h1>
        <p class="muted">近三个月上线的新歌</p>
      </div>
      <button
        ref="playAll"
        class="primary follow-release-play-all"
        :disabled="!latest.length || !playable"
        @click="emit('play', latest, 0)"
      >
        <Icon name="play" />
        <span>播放最新</span><span v-if="latest.length">{{ latest.length }}</span>
      </button>
    </header>
    <p v-if="notificationNotice" class="muted" role="status">{{ notificationNotice }}</p>
    <p v-if="latestError" class="muted" role="status">
      播放列表读取失败。
      <button type="button" class="text-button" :disabled="latestPending" @click="retryLatest">
        重试播放列表
      </button>
    </p>
    <p v-if="pending" role="status" class="muted">
      {{ initial ? '正在加载新歌发布…' : '正在加载更多发布…' }}
    </p>
    <div v-if="initial && error" class="catalog-error" role="alert">
      {{ error }}
      <button class="text-button" type="button" @click="more">重试</button>
    </div>
    <p
      v-if="!initial && !pending && !error && !releases.length && cursor === null"
      class="empty-state"
    >
      关注的歌手最近没有发布内容
    </p>

    <template
      v-for="(release, index) in releases"
      :key="`${group}:${release.kind}:${release.artistId}:${release.resourceId ?? ''}:${release.publishTime}:${index}`"
    >
      <p v-if="index === historyIndex" class="follow-release-day muted">以下是历史内容</p>
      <h2
        v-if="index === 0 || date(release.publishTime) !== date(releases[index - 1]!.publishTime)"
        class="follow-release-day"
      >
        {{ date(release.publishTime) }}
      </h2>
      <article class="follow-release" :aria-label="release.artistName">
        <header>
          <button
            class="follow-release-artist"
            type="button"
            @click="emit('artist', release.artistId)"
          >
            <img
              v-if="release.artistImageUrl"
              :src="release.artistImageUrl"
              alt=""
              loading="lazy"
            />
            <span>{{ release.artistName }}</span>
          </button>
          <p class="muted">
            {{ clock(release.publishTime) }} ·
            {{ release.kind === 'album' ? '专辑上线' : '单曲上线' }}
          </p>
          <FollowArtistNoticeMenu
            :epoch="epoch"
            :artist-id="release.artistId"
            :artist-name="release.artistName"
            @muted="hideMutedArtist"
            @expired="emit('expired')"
          />
        </header>

        <div v-if="release.kind === 'album' && release.albumId" class="follow-release-resource">
          <img
            v-if="release.resourceImageUrl"
            :src="release.resourceImageUrl"
            alt=""
            loading="lazy"
          />
          <button class="text-button" type="button" @click="emit('album', release.albumId)">
            {{ release.resourceName ?? '打开专辑' }}
          </button>
          <button
            class="icon-button"
            type="button"
            :disabled="!playable"
            :aria-label="`播放专辑 ${release.resourceName ?? ''}`"
            @click="emit('playAlbum', release.albumId)"
          >
            <Icon name="play" />
          </button>
        </div>
        <ul v-if="displayed(release, index).length" class="follow-release-tracks">
          <li
            v-for="(track, trackIndex) in displayed(release, index)"
            :key="`${track.song.id}:${trackIndex}`"
          >
            <div class="follow-release-resource">
              <img v-if="track.song.coverUrl" :src="track.song.coverUrl" alt="" loading="lazy" />
              <button
                class="text-button"
                type="button"
                :disabled="!playable"
                @click="playTrack(release, trackIndex, index)"
              >
                <Icon name="play" /> {{ track.song.name }}
              </button>
              <button
                v-if="track.mvId && release.kind === 'album'"
                class="text-button"
                type="button"
                :aria-label="`打开 ${track.song.name} 的 MV`"
                @click="emit('mv', track.mvId)"
              >
                MV
              </button>
            </div>
          </li>
        </ul>
        <div
          v-if="expanded === index && release.tracks.length > TRACK_WINDOW"
          class="follow-release-navigation"
        >
          <button
            class="text-button"
            type="button"
            :disabled="trackWindow === 0"
            @click="trackWindow--"
          >
            上一组歌曲
          </button>
          <button
            class="text-button"
            type="button"
            :disabled="(trackWindow + 1) * TRACK_WINDOW >= release.tracks.length"
            @click="trackWindow++"
          >
            下一组歌曲
          </button>
        </div>
        <button
          v-if="release.tracks.length > (release.kind === 'album' ? 0 : 1)"
          class="text-button follow-release-expand"
          type="button"
          :aria-expanded="expanded === index"
          @click="expand(index)"
        >
          {{ expanded === index ? '收起' : '展开'
          }}{{
            release.kind === 'album'
              ? `${release.albumSongCount ?? release.tracks.length} 首专辑歌曲`
              : `${release.tracks.length - 1} 首伴奏`
          }}
        </button>
      </article>
    </template>

    <div v-if="!initial && error" class="catalog-error" role="alert">
      {{ error }}
      <button class="text-button" type="button" @click="more">重试</button>
    </div>
    <nav
      v-if="cursor !== null || groups > 1"
      class="follow-release-navigation"
      aria-label="新歌发布分页"
    >
      <button
        v-if="group > 0"
        type="button"
        class="text-button"
        :disabled="pending"
        @click="pickGroup(group - 1)"
      >
        较新的发布
      </button>
      <button
        v-if="group + 1 < groups"
        type="button"
        class="text-button"
        :disabled="pending"
        @click="pickGroup(group + 1)"
      >
        较早的发布
      </button>
      <button
        v-else-if="cursor !== null"
        type="button"
        class="text-button"
        :disabled="pending || !!error"
        @click="more"
      >
        加载更多
      </button>
    </nav>
    <div v-if="autoMore && !pending && !error" ref="sentinel" aria-hidden="true"></div>
  </section>
</template>

<style scoped>
.follow-release-page {
  max-width: 980px;
  flex: 1;
  min-height: 0;
  min-width: 0;
  overflow: auto;
  overscroll-behavior: contain;
  padding-inline-end: 6px;
}
.follow-release-heading {
  display: flex;
  flex-wrap: wrap;
  gap: 12px;
  justify-content: space-between;
  align-items: center;
}
.follow-release-play-all {
  border-radius: 8px;
  padding-inline: 14px;
}
.follow-release-day {
  margin: 16px 0 10px;
  font-size: 14px;
}
.follow-release {
  border: 1px solid var(--line);
  border-radius: 10px;
  padding: 14px 18px;
  margin-block-end: 12px;
}
.follow-release header > p {
  margin-block: 6px 10px;
}
.follow-release-artist,
.follow-release-resource {
  display: flex;
  gap: 10px;
  align-items: center;
  min-width: 0;
}
.follow-release-artist {
  border: 0;
  background: none;
  color: inherit;
  cursor: pointer;
  font-weight: 600;
  padding: 0;
  text-align: start;
}
.follow-release-artist img {
  width: 36px;
  height: 36px;
  border-radius: 50%;
}
.follow-release-resource img {
  width: 48px;
  height: 48px;
  border-radius: 6px;
  object-fit: cover;
}
.follow-release-resource button {
  text-align: start;
}
.follow-release-tracks {
  list-style: none;
  padding: 0;
  margin: 0;
}
.follow-release-tracks li + li {
  margin-top: 8px;
}
.follow-release-expand {
  margin-block-start: 10px;
}
.follow-release-navigation {
  display: flex;
  justify-content: center;
  gap: 12px;
  flex-wrap: wrap;
  padding-block: 10px;
}
</style>
