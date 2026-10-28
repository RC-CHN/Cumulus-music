<script setup lang="ts">
import type { RankingCard, RankingGroup } from '../services/rankings';
import { movement } from '../services/rankings';
import Icon from './Icon.vue';
defineProps<{ groups: RankingGroup[]; playable: boolean }>();
defineEmits<{ open: [card: RankingCard]; play: [id: string] }>();
</script>
<template>
  <div class="ranking-groups">
    <section v-for="group in groups" :key="group.name" class="ranking-group">
      <h2>{{ group.name }}</h2>
      <div class="ranking-cards" :class="{ 'with-preview': group.displayType === 'TOP_3' }">
        <article
          v-for="card in group.cards"
          :key="card.id"
          class="ranking-card"
          :data-rank-id="card.id"
        >
          <button class="rank-open" :aria-label="`查看 ${card.name}`" @click="$emit('open', card)">
            <template v-if="group.displayType === 'TOP_3'">
              <span class="rank-heading">{{ card.name }} <Icon name="next" /></span>
              <small>{{ card.updateFrequency }}</small>
              <div class="rank-preview-content">
                <div class="rank-preview-covers">
                  <img
                    v-for="(track, index) in card.tracks.slice(0, 3)"
                    :key="track.id"
                    :src="track.coverUrl || undefined"
                    :style="{ '--cover-index': index }"
                    alt=""
                    loading="lazy"
                    referrerpolicy="no-referrer"
                  />
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
                  v-if="card.coverUrl"
                  :src="card.coverUrl"
                  alt=""
                  loading="lazy"
                  referrerpolicy="no-referrer"
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
            :aria-label="`播放榜单 ${card.name}`"
            :disabled="!playable"
            @click="$emit('play', card.id)"
          >
            <Icon name="play" />
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
.rank-name {
  display: block;
  padding-top: 9px;
  overflow: hidden;
  white-space: nowrap;
  text-overflow: ellipsis;
}
.rank-name.double {
  white-space: normal;
}
.rank-play {
  position: absolute;
  right: 10px;
  top: 10px;
  background: var(--surface-raised);
  border-radius: 50%;
  width: 30px;
  height: 30px;
}
.with-preview .ranking-card {
  border-radius: 10px;
  background: var(--surface-alt);
}
.with-preview .rank-open {
  padding: 16px 20px 19px;
}
.rank-heading {
  font-size: 18px;
  font-weight: 650;
  display: flex;
  align-items: center;
  gap: 10px;
  padding-right: 22px;
}
.rank-heading .icon {
  width: 12px;
  height: 12px;
}
.rank-open > small {
  display: block;
  margin-top: 6px;
  color: var(--text-secondary);
  font-size: 11px;
}
.rank-preview-content {
  display: flex;
  gap: 16px;
  margin-top: 16px;
  align-items: center;
}
.rank-preview-covers {
  position: relative;
  width: 98px;
  min-width: 98px;
  height: 78px;
}
.rank-preview-covers img {
  position: absolute;
  width: calc(78px - var(--cover-index) * 20px);
  height: calc(78px - var(--cover-index) * 20px);
  top: calc(var(--cover-index) * 10px);
  left: calc(var(--cover-index) * 30px);
  z-index: calc(3 - var(--cover-index));
  border-radius: 8px;
  object-fit: cover;
  background: var(--surface-hover);
}
.rank-preview-rows {
  flex: 1;
  min-width: 0;
}
.rank-preview-row {
  display: flex;
  align-items: center;
  gap: 8px;
  height: 26px;
  font-size: 12px;
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
}
</style>
