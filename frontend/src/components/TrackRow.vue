<script setup lang="ts">
import { computed, ref } from 'vue';
import { usePlaybackTriggerFocus } from '../composables/usePlaybackTriggerFocus';
import SearchMatchText from './search/SearchMatchText.vue';
import SearchSongTitleHint from './search/SearchSongTitleHint.vue';
import type { SearchSongInlineCredits, SearchSongTableHint } from '../services/search';
import type {
  TrackCurrentToggle,
  TrackMenuRequest,
  TrackPendingToggle,
  TrackPlayGesture,
} from '../services/trackMenu';
import type { TrackView } from '../services/contracts';
import { artworkAt } from '../services/artwork';
import Icon from './Icon.vue';
const props = defineProps<{
  track: TrackView;
  matchKeyword?: string;
  searchTitleHint?: SearchSongTableHint;
  searchInlineCredits?: SearchSongInlineCredits;
  originalRecording?: boolean;
  index: number;
  liked: boolean;
  busy: boolean;
  playable?: boolean;
  currentId?: string;
  currentToggle?: TrackCurrentToggle;
  pendingToggle?: TrackPendingToggle;
  toggleBusy?: boolean;
  navigable?: boolean;
  readOnly?: boolean;
  selection?: boolean;
  showPopularity?: boolean;
}>();
const emit = defineEmits<{
  like: [track: TrackView];
  play: [index: number, gesture?: TrackPlayGesture];
  toggleCurrent: [target: TrackCurrentToggle, anchor: HTMLButtonElement];
  togglePending: [target: TrackPendingToggle, anchor: HTMLButtonElement];
  more: [request: TrackMenuRequest];
  artist: [id: string];
  album: [id: string];
}>();
const cellRole = computed(() => (props.selection === undefined ? 'cell' : 'gridcell'));
const shownToggle = computed(() => props.pendingToggle ?? props.currentToggle);
const root = ref<HTMLElement>();
const track = props.track;
const artistSuffixes = computed(() => {
  const suffixes = props.searchInlineCredits?.artistSuffixes;
  return suffixes?.length === props.track.artistRefs?.length ? suffixes : undefined;
});
const primaryDisabled = computed(
  () =>
    props.track.missing ||
    (props.pendingToggle
      ? props.pendingToggle.disabled
      : !props.playable || (!!props.currentToggle && props.toggleBusy)),
);
const playbackFocus = usePlaybackTriggerFocus(
  () => !primaryDisabled.value,
  root,
  () =>
    props.track === track &&
    !!root.value?.isConnected &&
    !root.value.closest('[inert], .page-leave-active'),
  // A newly selected row can become the current row before select's IPC
  // completes. Do not consume its focus owner at that intermediate readiness.
  () => !props.toggleBusy,
);

function primary(event: MouseEvent) {
  const anchor = event.currentTarget as HTMLButtonElement;
  playbackFocus.start(() => {
    if (props.pendingToggle) emit('togglePending', props.pendingToggle, anchor);
    else if (props.currentToggle) emit('toggleCurrent', props.currentToggle, anchor);
    else emit('play', props.index);
  }, anchor);
}
function more(event: MouseEvent | KeyboardEvent) {
  if (!props.readOnly)
    emit('more', { track: props.track, event, play: () => emit('play', props.index) });
}
function duration(ms: number) {
  const seconds = Math.floor(ms / 1000);
  return `${Math.floor(seconds / 60)
    .toString()
    .padStart(2, '0')}:${(seconds % 60).toString().padStart(2, '0')}`;
}
</script>
<template>
  <div
    ref="root"
    class="track-columns track-row"
    :class="{
      unavailable: track.missing,
      current: !track.missing && (track.local ? !!currentToggle : track.id === currentId),
      striped: index % 2 === 0,
    }"
    @dblclick="!track.missing && playable && emit('play', index, 'double_click')"
    @contextmenu.prevent.stop="more"
    @keydown.shift.f10.prevent.stop="more"
    role="row"
    :aria-selected="selection"
    :aria-rowindex="index + 2"
    :data-track-id="track.id"
  >
    <span class="track-index" :role="cellRole"
      ><button
        class="track-play icon-button"
        data-result-action="play"
        :aria-label="`${shownToggle ? (shownToggle.pause ? '暂停' : '继续播放') : '播放'} ${track.name}`"
        :disabled="primaryDisabled"
        @click.stop="primary"
        @dblclick.stop
      >
        <span>{{ (index + 1).toString().padStart(2, '0') }}</span
        ><Icon
          :name="
            shownToggle
              ? shownToggle.pause
                ? 'pause'
                : 'play'
              : !track.missing && !track.local && track.id === currentId
                ? 'volume'
                : 'play'
          "
        /></button
    ></span>
    <div class="track-title" :role="cellRole">
      <img
        v-if="!readOnly && track.coverUrl"
        :src="artworkAt(track.coverUrl, 120)"
        alt=""
        loading="lazy"
        decoding="async"
        referrerpolicy="no-referrer"
      /><span v-else class="track-art"><Icon name="music" /></span>
      <div>
        <div
          class="ellipsis"
          :class="{ 'track-title-with-original': originalRecording }"
          :data-tooltip="track.name"
        >
          <span class="track-row-title-text">
            <SearchMatchText v-if="matchKeyword" :text="track.name" :keyword="matchKeyword" />
            <template v-else>{{ track.name }}</template>
            <span
              v-if="searchInlineCredits?.titleSuffix?.length"
              class="track-title-inline-credit"
              data-search-inline-credit="title"
              ><SearchMatchText :parts="searchInlineCredits.titleSuffix"
            /></span>
          </span>
          <span v-if="track.fee === 1" class="vip-tag">VIP</span>
          <span v-if="originalRecording" class="search-original-tag">原唱</span>
        </div>
        <small
          v-if="navigable && track.artistRefs?.length"
          class="ellipsis track-credit-links"
          :data-tooltip="track.artistRefs.map((artist) => artist.name).join(' / ')"
          ><template v-for="(artist, i) in track.artistRefs" :key="i"
            ><span v-if="i"> / </span
            ><button
              v-if="artist.id"
              :class="{ ellipsis: artistSuffixes?.[i] }"
              :data-result-action="`artist:${artist.id}`"
              @click.stop="emit('artist', artist.id)"
              @dblclick.stop
            >
              <SearchMatchText v-if="matchKeyword" :text="artist.name" :keyword="matchKeyword" />
              <template v-else>{{ artist.name }}</template>
              <span v-if="artistSuffixes?.[i]" data-search-inline-credit="artist"
                ><SearchMatchText :parts="artistSuffixes[i]"
              /></span>
            </button>
            <span v-else>
              <SearchMatchText v-if="matchKeyword" :text="artist.name" :keyword="matchKeyword" />
              <template v-else>{{ artist.name }}</template>
            </span></template
          ></small
        >
        <small v-else class="ellipsis" :data-tooltip="track.artists.join(' / ')">
          <SearchMatchText
            v-if="matchKeyword && track.artists.length"
            :text="track.artists.join(' / ')"
            :keyword="matchKeyword"
          />
          <template v-else>{{
            track.artists.join(' / ') || (track.local ? '本地文件' : '未知歌手')
          }}</template>
        </small>
        <SearchSongTitleHint v-if="searchTitleHint" :hint="searchTitleHint" />
      </div>
    </div>
    <span
      v-if="!showPopularity"
      class="track-album ellipsis"
      :role="cellRole"
      :data-tooltip="track.album"
      ><button
        v-if="navigable && track.albumId"
        class="track-album-link ellipsis"
        data-result-action="album"
        @click.stop="emit('album', track.albumId)"
        @dblclick.stop
      >
        <SearchMatchText v-if="matchKeyword" :text="track.album" :keyword="matchKeyword" />
        <template v-else>{{ track.album }}</template>
        <span v-if="searchInlineCredits?.albumSuffix?.length" data-search-inline-credit="album"
          ><SearchMatchText :parts="searchInlineCredits.albumSuffix"
        /></span>
      </button>
      <template v-else>
        <SearchMatchText v-if="matchKeyword" :text="track.album" :keyword="matchKeyword" />
        <template v-else>{{ track.album }}</template>
        <span v-if="searchInlineCredits?.albumSuffix?.length" data-search-inline-credit="album"
          ><SearchMatchText :parts="searchInlineCredits.albumSuffix"
        /></span> </template
    ></span>
    <span :role="cellRole"
      ><button
        class="icon-button like-button"
        data-result-action="like"
        :class="{ liked: liked }"
        :aria-label="`${liked ? '取消喜欢' : '喜欢'} ${track.name}`"
        :aria-pressed="liked"
        :disabled="readOnly || track.missing || track.local || busy"
        @click.stop="emit('like', track)"
        @dblclick.stop
      >
        <Icon name="heart" /></button
    ></span>
    <span class="track-time-actions" :role="cellRole"
      ><span class="track-duration">{{ track.missing ? '—' : duration(track.durationMs) }}</span
      ><button
        v-if="!readOnly"
        class="icon-button track-more"
        data-result-action="more"
        :aria-label="`更多操作 ${track.name}`"
        aria-haspopup="menu"
        @click.stop="more"
        @dblclick.stop
      >
        <Icon name="more" /></button
    ></span>
    <span v-if="showPopularity" :role="cellRole">
      <span
        class="track-popularity"
        role="meter"
        aria-label="热度"
        aria-valuemin="0"
        aria-valuemax="100"
        :aria-valuenow="track.albumPopularity ?? 0"
      >
        <span :style="{ width: `${track.albumPopularity ?? 0}%` }" />
      </span>
    </span>
  </div>
</template>

<style scoped>
.track-popularity {
  display: block;
  width: min(100%, 60px);
  height: 4px;
  border-radius: 2px;
  background: var(--line);
}
.track-popularity span {
  display: block;
  height: 100%;
  border-radius: inherit;
  background: var(--accent);
  opacity: 0.7;
}
</style>
