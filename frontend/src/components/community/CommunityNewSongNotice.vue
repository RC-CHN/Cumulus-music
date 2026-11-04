<script setup lang="ts">
import type { CommunityNoticeCard } from '../../services/community';
import Icon from '../Icon.vue';

defineProps<{ card: CommunityNoticeCard }>();
const emit = defineEmits<{ open: [] }>();
</script>

<template>
  <button
    class="new-song-notice"
    :class="{ 'new-song-notice-artwork': card.style === 'artists' && card.avatars[0] }"
    type="button"
    data-community-opener="new-song-notice"
    @click="emit('open')"
  >
    <img
      v-if="card.style === 'artists' && card.avatars[0]"
      class="new-song-notice-background"
      :src="card.avatars[0]"
      alt=""
    />
    <span class="new-song-notice-leading" aria-hidden="true">
      <template v-if="card.style === 'artists' && card.avatars.length">
        <img v-for="(avatar, index) in card.avatars" :key="index" :src="avatar" alt="" />
      </template>
      <Icon v-else name="music" />
    </span>
    <span class="new-song-notice-text">{{ card.text || '关注歌手的新歌发布' }}</span>
    <img
      v-if="card.style === 'musicians' && card.cover"
      class="new-song-notice-cover"
      :src="card.cover"
      alt=""
    />
    <Icon class="new-song-notice-next" name="forward" />
  </button>
</template>

<style scoped>
.new-song-notice {
  display: flex;
  align-items: center;
  width: 100%;
  min-height: 62px;
  padding: 12px 16px;
  margin-block: 4px 20px;
  gap: 12px;
  border-radius: 10px;
  isolation: isolate;
  position: relative;
  overflow: hidden;
  background: var(--surface-hover);
  text-align: start;
}
.new-song-notice-artwork {
  min-height: 68px;
  padding: 20px;
  background: #303039;
  color: #fff;
}
.new-song-notice-background {
  position: absolute;
  z-index: -1;
  inset: 0;
  width: 100%;
  height: 100%;
  object-fit: cover;
  transform: scale(1.2);
  filter: blur(50px) brightness(0.78);
  opacity: 0.5;
}
.new-song-notice-leading {
  display: flex;
  flex: none;
  align-items: center;
}
.new-song-notice-leading img {
  width: 28px;
  height: 28px;
  border-radius: 50%;
  object-fit: cover;
}
.new-song-notice-leading img + img {
  margin-inline-start: -6px;
}
.new-song-notice-text {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 15px;
}
.new-song-notice-cover {
  width: 44px;
  height: 44px;
  border-radius: 4px;
  object-fit: cover;
}
.new-song-notice-next {
  flex: none;
}
</style>
