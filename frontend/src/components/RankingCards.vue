<script setup lang="ts">
import { ref } from 'vue';
import type { RankingCard, RankingGroup } from '../services/rankings';
import { movement } from '../services/rankings';
import Icon from './Icon.vue';
import type { PlaybackStatus } from '../services/player';
const props = defineProps<{
  groups: RankingGroup[];
  playable: boolean;
  hideGroupHeading?: boolean;
  focusKeyPrefix?: string;
  currentPlaylistId?: string;
  playbackState?: PlaybackStatus;
}>();
defineEmits<{ open: [card: RankingCard]; play: [id: string] }>();
const failedCovers = ref(new Set<string>());
function active(id: string) {
  return (
    props.currentPlaylistId === id && ['playing', 'buffering'].includes(props.playbackState ?? '')
  );
}
function label(card: RankingCard) {
  const action = active(card.id)
    ? '暂停榜单'
    : props.currentPlaylistId === card.id && props.playbackState === 'paused'
      ? '继续播放榜单'
      : '播放榜单';
  return `${action} ${card.name}`;
}
</script>
<template>
  <div class="ranking-groups">
    <section v-for="group in groups" :key="group.name" class="ranking-group">
      <h2 v-if="!hideGroupHeading">{{ group.name }}</h2>
      <div class="ranking-cards" :class="{ 'with-preview': group.displayType === 'TOP_3' }">
        <article
          v-for="card in group.cards"
          :key="card.id"
          class="ranking-card"
          :data-rank-id="card.id"
          :style="{ '--rank-caption-height': card.nameShowStyle === 'doubleRow' ? '49px' : '29px' }"
        >
          <button
            class="rank-open"
            data-rank-action="open"
            :data-home-card="focusKeyPrefix ? focusKeyPrefix + card.id : undefined"
            :aria-label="`查看 ${card.name}`"
            @click="$emit('open', card)"
          >
            <template v-if="group.displayType === 'TOP_3'">
              <span class="rank-title-row">
                <span class="rank-heading" :title="card.name">{{ card.name }}</span>
                <small :title="card.updateFrequency">{{ card.updateFrequency }}</small>
              </span>
              <div class="rank-preview-content">
                <div class="rank-preview-covers">
                  <span
                    v-for="(track, index) in card.tracks.slice(0, 3)"
                    :key="track.id"
                    class="rank-preview-image"
                    :style="{ '--cover-index': index }"
                  >
                    <img
                      v-if="track.coverUrl && !failedCovers.has(track.coverUrl)"
                      :src="track.coverUrl"
                      alt=""
                      loading="lazy"
                      referrerpolicy="no-referrer"
                      @error="failedCovers.add(track.coverUrl)"
                    /><Icon v-else name="music" />
                  </span>
                </div>
                <div class="rank-preview-rows">
                  <div
                    v-for="(track, index) in card.tracks.slice(0, 3)"
                    :key="track.id"
                    class="rank-preview-row"
                  >
                    <span class="rank-number">{{ index + 1 }}</span
                    ><span class="ellipsis"
                      >{{ track.name }} <small>— {{ track.artist }}</small></span
                    >
                    <span
                      class="rank-movement"
                      :class="{ rise: movement(track) === '↑', fresh: movement(track) === '新' }"
                      >{{ movement(track) }}</span
                    >
                  </div>
                  <small v-if="!card.tracks.length">暂无上榜内容</small>
                </div>
              </div>
            </template>
            <template v-else>
              <div class="rank-cover">
                <img
                  v-if="card.coverUrl && !failedCovers.has(card.coverUrl)"
                  :src="card.coverUrl"
                  alt=""
                  loading="lazy"
                  referrerpolicy="no-referrer"
                  @error="failedCovers.add(card.coverUrl)"
                /><Icon v-else name="music" />
                <small v-if="!card.textCover">{{ card.updateFrequency }}</small>
              </div>
              <span class="rank-name" :class="{ double: card.nameShowStyle === 'doubleRow' }">{{
                card.name
              }}</span>
            </template>
          </button>
          <button
            v-if="card.canPlay && card.target === 'playlist'"
            class="rank-play icon-button"
            :class="{ playing: active(card.id) }"
            data-rank-action="play"
            :aria-label="label(card)"
            :title="label(card)"
            :aria-pressed="active(card.id)"
            :disabled="!playable"
            @click="$emit('play', card.id)"
          >
            <Icon :name="active(card.id) ? 'pause' : 'play'" />
          </button>
        </article>
      </div>
    </section>
    <p v-if="!groups.length" class="empty-state">暂无榜单</p>
  </div>
</template>
<style scoped>
.ranking-group h2 {
  font-size: 20px;
  margin: 26px 0 16px;
}
.ranking-cards {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(138px, 1fr));
  gap: 22px 18px;
}
.ranking-cards.with-preview {
  grid-template-columns: repeat(auto-fit, minmax(min(100%, 330px), 1fr));
}
.ranking-card {
  min-width: 0;
  position: relative;
}
.rank-open {
  width: 100%;
  border: 0;
  padding: 0;
  background: transparent;
  text-align: left;
  color: inherit;
  cursor: pointer;
}
.rank-cover {
  position: relative;
  aspect-ratio: 1;
  border-radius: 10px;
  overflow: hidden;
  background: var(--surface-alt);
  display: grid;
  place-items: center;
}
.rank-cover img {
  width: 100%;
  height: 100%;
  object-fit: cover;
}
.rank-cover small {
  position: absolute;
  top: 10px;
  left: 10px;
  color: var(--on-accent);
  text-shadow: 0 1px 4px #333;
}
.rank-cover:has(> .icon) small {
  color: var(--text-secondary);
  text-shadow: none;
}
.rank-name {
  display: block;
  padding-top: 9px;
  overflow: hidden;
  white-space: nowrap;
  text-overflow: ellipsis;
  height: 29px;
  line-height: 20px;
}
.rank-name.double {
  white-space: normal;
  height: 49px;
  display: -webkit-box;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
}
.rank-play {
  position: absolute;
  z-index: 5;
  right: 10px;
  top: calc(100% - var(--rank-caption-height) - 35px);
  background: var(--surface-raised);
  border-radius: 50%;
  width: 30px;
  height: 30px;
  opacity: 0;
  pointer-events: none;
  transition: opacity 200ms ease;
}
.rank-play:disabled {
  opacity: 0;
}
.rank-play.playing,
.ranking-card:hover .rank-play,
.ranking-card:focus-within .rank-play {
  opacity: 1;
  pointer-events: auto;
}
.rank-play.playing:disabled,
.ranking-card:hover .rank-play:disabled,
.ranking-card:focus-within .rank-play:disabled {
  opacity: 0.4;
}
.with-preview .ranking-card {
  border-radius: 10px;
  background: var(--surface-alt);
  border: 1px solid var(--line);
  box-shadow: 0 6px 16px rgb(40 50 72 / 4%);
}
.with-preview .rank-open {
  padding: 16px 20px 19px;
}
.with-preview .rank-play {
  left: 59px;
  top: 94px;
  bottom: auto;
  transform: translate(-50%, -50%);
}
.rank-title-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  height: 24px;
  gap: 12px;
}
.rank-title-row > small {
  flex-shrink: 0;
  max-width: 45%;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: var(--muted);
  font-size: 12px;
}
.rank-heading {
  font-size: 18px;
  font-weight: 400;
  display: block;
  min-width: 0;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.rank-preview-content {
  display: flex;
  gap: 14px;
  margin-top: 15px;
  align-items: center;
}
.rank-preview-covers {
  position: relative;
  width: 94px;
  min-width: 94px;
  height: 78px;
}
.rank-preview-image {
  position: absolute;
  width: calc(78px - var(--cover-index) * 20px);
  height: calc(78px - var(--cover-index) * 20px);
  bottom: 0;
  left: calc(var(--cover-index) * 28px);
  z-index: calc(3 - var(--cover-index));
  border-radius: 6px;
  overflow: hidden;
  background: var(--surface-hover);
  box-shadow: 0 4px 10px rgb(0 0 0 / 10%);
  display: grid;
  place-items: center;
}
.rank-preview-image:nth-child(3) {
  opacity: 0.6;
}
.rank-preview-image img {
  width: 100%;
  height: 100%;
  object-fit: cover;
}
.rank-preview-rows {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  justify-content: space-between;
  height: 78px;
}
.rank-preview-row {
  display: flex;
  align-items: center;
  gap: 8px;
  height: 20px;
  font-size: 15px;
}
.rank-preview-row > .ellipsis {
  flex: 1;
  min-width: 0;
}
.rank-preview-row small {
  color: var(--muted);
}
.rank-number {
  color: var(--text-secondary);
  width: 12px;
}
.rank-movement {
  color: var(--muted);
  min-width: 12px;
  font-size: 11px;
}
.rank-movement.rise {
  color: var(--accent);
}
.rank-movement.fresh {
  color: var(--success);
  font-size: 15px;
}
@media (prefers-reduced-motion: reduce) {
  .rank-play {
    transition: none;
  }
}
</style>
