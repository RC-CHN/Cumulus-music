<script setup lang="ts">
import type { MemberCard, MemberTarget } from '../services/member';
defineProps<{ cards: MemberCard[]; banners?: boolean; busy?: boolean }>();
const emit = defineEmits<{ open: [target: MemberTarget] }>();
</script>
<template>
  <div class="member-cards" :class="{ banners }">
    <button
      v-for="(card, i) in cards"
      :key="`${i}:${card.id}`"
      class="member-card"
      :data-member-target="card.action?.kind"
      :data-member-id="card.action && 'id' in card.action ? card.action.id : undefined"
      :disabled="!card.action || busy"
      :aria-label="`${card.action?.kind === 'external' ? '在浏览器打开' : '打开'} ${card.title || '会员推荐'}`"
      @click="card.action && emit('open', card.action)"
    >
      <img
        v-if="card.coverUrl"
        :src="card.coverUrl"
        alt=""
        loading="lazy"
        referrerpolicy="no-referrer"
      /><span v-else class="member-cover-placeholder">♫</span>
      <strong v-if="!banners">{{ card.title }}</strong
      ><small v-if="!banners && card.subtitle">{{ card.subtitle }}</small>
    </button>
  </div>
</template>
<style scoped>
.member-cards {
  display: flex;
  gap: 16px;
  overflow-x: auto;
  padding-bottom: 10px;
  scroll-snap-type: x proximity;
  min-width: 0;
}
.member-card {
  display: flex;
  flex: 0 0 144px;
  min-width: 0;
  flex-direction: column;
  gap: 7px;
  text-align: left;
  background: transparent;
  border: 0;
  padding: 0;
  color: inherit;
  scroll-snap-align: start;
  cursor: pointer;
}
.member-card img,
.member-cover-placeholder {
  width: 144px;
  height: 144px;
  object-fit: cover;
  border-radius: 10px;
}
.member-cover-placeholder {
  display: grid;
  place-items: center;
  background: var(--surface, #f7f7f9);
  font-size: 32px;
}
.member-card strong {
  font-size: 13px;
  font-weight: 500;
  line-height: 1.5;
  display: -webkit-box;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
  overflow: hidden;
}
.member-card small {
  font-size: 11px;
  opacity: 0.6;
  max-width: 100%;
  overflow: hidden;
  white-space: nowrap;
  text-overflow: ellipsis;
}
.member-card:disabled {
  cursor: default;
}
.banners .member-card {
  flex-basis: 100%;
}
.banners .member-card img {
  width: 100%;
  height: auto;
  aspect-ratio: 2.5714;
  object-fit: cover;
}
.banners {
  padding: 0;
}
.member-card:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: 2px;
}
</style>
