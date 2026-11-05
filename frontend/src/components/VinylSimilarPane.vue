<script setup lang="ts">
import { computed, nextTick, ref, watch } from 'vue';
import { artworkAt } from '../services/artwork';
import { playlistTableCount } from '../services/playlistHeader';
import type {
  VinylRelatedPlaylist,
  VinylSimilarAction,
  VinylSimilarLane,
  VinylSimilarReadState,
  VinylSimilarSong,
} from '../services/vinylSimilar';
import Icon from './Icon.vue';

const props = defineProps<{
  playId: string;
  embedded?: boolean;
  songs: readonly VinylSimilarSong[];
  playlists: readonly VinylRelatedPlaylist[];
  songsState: VinylSimilarReadState;
  playlistsState: VinylSimilarReadState;
  notice?: string;
  pending: boolean;
  expanded: boolean;
  canPlay: boolean;
}>();
const emit = defineEmits<{
  play: [action: VinylSimilarAction];
  retry: [lane: VinylSimilarLane];
  openPlaylist: [id: string];
  expand: [];
  openArtist: [id: string];
}>();
const scroll = ref<HTMLElement>();
const failedImages = ref(new Set<string>());
const lanes = computed<readonly VinylSimilarLane[]>(() =>
  props.embedded ? ['songs', 'playlists'] : ['playlists', 'songs'],
);
const playlistLimit = 6;
const visiblePlaylists = computed(() =>
  props.expanded ? props.playlists : props.playlists.slice(0, playlistLimit),
);
const laneState = (state: VinylSimilarReadState, size: number) =>
  state === 'ready' && size === 0 ? 'empty' : state;
const paneState = computed(() => {
  const states = [props.playlistsState, props.songsState];
  if (states.includes('error')) return 'error';
  if (states.includes('loading')) return 'loading';
  if (states.includes('idle')) return 'idle';
  return props.playlists.length || props.songs.length ? 'ready' : 'empty';
});
const mayPlay = computed(() => props.canPlay && !props.pending && props.songsState === 'ready');
const mayPlayHeader = computed(() => mayPlay.value && props.songs[0]?.playable === true);

function playHeader() {
  if (mayPlayHeader.value) emit('play', { kind: 'header' });
}
function playSong(kind: 'cover' | 'double_click', index: number) {
  if (mayPlay.value && props.songs[index]?.playable) emit('play', { kind, index });
}
function playLabel(song: VinylSimilarSong) {
  const title = song.track.name || '未知歌曲';
  return song.playable ? `播放 ${title}` : `${title} 暂无可用音源`;
}
function artKey(kind: 'playlist' | 'song', id: string, url: string | null) {
  return `${kind}:${id}:${url ?? ''}`;
}
function art(kind: 'playlist' | 'song', id: string, url: string | null) {
  if (!url || failedImages.value.has(artKey(kind, id, url))) return undefined;
  return artworkAt(url, kind === 'playlist' ? 240 : 120);
}
function hideImage(kind: 'playlist' | 'song', id: string, url: string | null) {
  failedImages.value.add(artKey(kind, id, url));
}
function playCount(playlist: VinylRelatedPlaylist) {
  return playlist.playCount > 0 ? playlistTableCount(playlist.playCount, false) : null;
}
function artists(song: VinylSimilarSong) {
  const refs = song.track.artistRefs?.filter((artist) => artist.name);
  if (refs?.length) return refs;
  return (song.track.artists.length ? song.track.artists : ['未知歌手']).map((name) => ({
    id: null,
    name,
  }));
}
async function retryLane(lane: VinylSimilarLane, event: MouseEvent) {
  const focus = event.currentTarget === document.activeElement;
  const origin = props.playId;
  emit('retry', lane);
  await nextTick();
  if (focus && origin === props.playId && document.activeElement === document.body) {
    scroll.value?.focus({ preventScroll: true });
  }
}
async function expandPlaylists(event: MouseEvent) {
  const focus = event.currentTarget === document.activeElement;
  const origin = props.playId;
  emit('expand');
  await nextTick();
  if (
    focus &&
    origin === props.playId &&
    props.expanded &&
    document.activeElement === document.body
  ) {
    scroll.value
      ?.querySelectorAll<HTMLButtonElement>('[data-vinyl-similar-playlist]')
      [playlistLimit]?.focus();
  }
}
watch(
  () => props.embedded,
  async () => {
    const focused = document.activeElement;
    if (!(focused instanceof HTMLElement) || !scroll.value?.contains(focused)) return;
    await nextTick();
    // Moving keyed lanes can release DOM focus. Preserve the same control,
    // while leaving a new focus chosen by the parent or user untouched.
    if (document.activeElement === document.body && scroll.value?.contains(focused)) {
      focused.focus({ preventScroll: true });
    }
  },
);
watch(
  () => props.playId,
  () => {
    failedImages.value.clear();
    if (!props.embedded && scroll.value) scroll.value.scrollTop = 0;
  },
);
</script>

<template>
  <div
    v-show="!embedded || paneState !== 'empty'"
    ref="scroll"
    class="vinyl-similar-pane"
    :class="{ 'vinyl-similar-embedded': embedded }"
    data-vinyl-similar
    tabindex="-1"
    aria-label="相关推荐"
    :data-vinyl-similar-state="paneState"
    @wheel="!embedded && $event.stopPropagation()"
  >
    <template v-for="lane in lanes" :key="lane">
      <section
        v-if="lane === 'playlists'"
        v-show="playlistsState !== 'ready' || playlists.length > 0"
        class="vinyl-similar-card"
        data-vinyl-similar-playlists
        :data-vinyl-similar-state="laneState(playlistsState, playlists.length)"
        :aria-busy="playlistsState === 'loading'"
        aria-label="相关歌单"
      >
        <h2>相关歌单</h2>
        <div v-if="playlistsState === 'error'" class="vinyl-similar-message" role="status">
          <span>相关歌单加载失败</span>
          <button
            type="button"
            data-vinyl-similar-retry="playlists"
            @click="retryLane('playlists', $event)"
          >
            重试
          </button>
        </div>
        <p
          v-else-if="playlistsState === 'idle' || playlistsState === 'loading'"
          class="vinyl-similar-message"
          role="status"
        >
          正在加载相关歌单…
        </p>
        <ul v-else class="vinyl-similar-playlist-grid">
          <li v-for="(playlist, index) in visiblePlaylists" :key="`${index}:${playlist.id}`">
            <button
              type="button"
              class="vinyl-similar-playlist"
              :data-vinyl-similar-playlist="playlist.id"
              :aria-label="`查看歌单：${playlist.name}`"
              :aria-description="playCount(playlist)?.label"
              :title="playlist.name"
              @click="emit('openPlaylist', playlist.id)"
            >
              <span class="vinyl-similar-playlist-art">
                <img
                  v-if="art('playlist', playlist.id, playlist.coverUrl)"
                  :src="art('playlist', playlist.id, playlist.coverUrl)"
                  alt=""
                  loading="lazy"
                  decoding="async"
                  referrerpolicy="no-referrer"
                  @error="hideImage('playlist', playlist.id, playlist.coverUrl)"
                />
                <Icon v-else name="list" />
                <span
                  v-if="playCount(playlist)"
                  class="vinyl-similar-play-count"
                  :title="playCount(playlist)?.label"
                >
                  <Icon name="play" />{{ playCount(playlist)?.text }}
                </span>
              </span>
              <span class="vinyl-similar-playlist-title">{{ playlist.name }}</span>
            </button>
          </li>
        </ul>
        <button
          v-if="playlistsState === 'ready' && !expanded && playlists.length > playlistLimit"
          type="button"
          class="vinyl-similar-expand"
          data-vinyl-similar-expand
          @click="expandPlaylists"
        >
          展开更多
        </button>
      </section>

      <section
        v-else
        v-show="songsState !== 'ready' || songs.length > 0"
        class="vinyl-similar-card"
        data-vinyl-similar-songs
        :data-vinyl-similar-state="laneState(songsState, songs.length)"
        :aria-busy="songsState === 'loading' || pending"
        aria-label="相似歌曲"
      >
        <header class="vinyl-similar-heading">
          <h2>相似歌曲</h2>
          <button
            v-if="songsState === 'ready' && songs.length"
            type="button"
            class="vinyl-similar-play-all"
            aria-label="播放相似歌曲"
            :aria-disabled="!mayPlayHeader"
            :aria-description="
              songs[0]?.playable === false ? '第一首相似歌曲暂无可用音源' : undefined
            "
            data-vinyl-similar-play-all
            @click="playHeader"
          >
            <Icon name="play" />播放
          </button>
        </header>
        <p
          v-if="notice || pending"
          class="vinyl-similar-message"
          data-vinyl-similar-notice
          role="status"
        >
          <span>{{ notice || '正在请求播放…' }}</span>
          <button
            v-if="notice && !pending && songsState === 'ready'"
            type="button"
            data-vinyl-similar-retry="songs"
            @click="retryLane('songs', $event)"
          >
            重新读取
          </button>
        </p>
        <div v-if="songsState === 'error'" class="vinyl-similar-message" role="status">
          <span>相似歌曲加载失败</span>
          <button
            type="button"
            data-vinyl-similar-retry="songs"
            @click="retryLane('songs', $event)"
          >
            重试
          </button>
        </div>
        <p
          v-else-if="songsState === 'idle' || songsState === 'loading'"
          class="vinyl-similar-message"
          role="status"
        >
          正在加载相似歌曲…
        </p>
        <ul v-else class="vinyl-similar-track-list">
          <li
            v-for="(song, index) in songs"
            :key="`${index}:${song.track.id}`"
            class="vinyl-similar-track"
            :class="{ 'vinyl-similar-track-denied': !song.playable }"
            :data-vinyl-similar-track="song.track.id"
            @dblclick="playSong('double_click', index)"
          >
            <button
              type="button"
              class="vinyl-similar-track-art"
              :aria-label="playLabel(song)"
              :aria-disabled="!mayPlay || !song.playable"
              data-vinyl-similar-track-art
              @click.stop="$event.detail <= 1 && playSong('cover', index)"
              @dblclick.stop
            >
              <img
                v-if="art('song', song.track.id, song.track.coverUrl)"
                :src="art('song', song.track.id, song.track.coverUrl)"
                alt=""
                loading="lazy"
                decoding="async"
                referrerpolicy="no-referrer"
                @error="hideImage('song', song.track.id, song.track.coverUrl)"
              />
              <Icon v-else name="music" />
              <span class="vinyl-similar-track-mask"><Icon name="play" /></span>
            </button>
            <div class="vinyl-similar-track-copy">
              <button
                type="button"
                class="vinyl-similar-track-title"
                data-vinyl-similar-track-title
                :aria-label="playLabel(song)"
                :aria-disabled="!mayPlay || !song.playable"
                :title="song.track.name"
                @click="$event.detail === 0 && playSong('double_click', index)"
              >
                {{ song.track.name || '未知歌曲' }}
              </button>
              <span class="vinyl-similar-track-credit">
                <template v-for="(artist, artistIndex) in artists(song)" :key="artistIndex">
                  <span v-if="artistIndex" aria-hidden="true"> / </span>
                  <button
                    v-if="artist.id"
                    type="button"
                    :aria-label="`查看歌手：${artist.name}`"
                    @click.stop="emit('openArtist', artist.id)"
                    @dblclick.stop
                  >
                    {{ artist.name }}
                  </button>
                  <span v-else>{{ artist.name }}</span>
                </template>
              </span>
            </div>
          </li>
        </ul>
      </section>
    </template>
    <p v-if="!embedded && paneState === 'empty'" class="vinyl-similar-message" role="status">
      暂无相关推荐
    </p>
  </div>
</template>

<style scoped src="../styles/vinyl-similar.css"></style>
