<script setup lang="ts">
import type { ArtistCard } from '../../services/catalog';
import Icon from '../Icon.vue';
defineProps<{
  artist: ArtistCard;
  profileAccountId?: string | null;
  isSelf?: boolean;
  followed: boolean | null;
  followDisabled: boolean;
  playDisabled: boolean;
  refreshDisabled: boolean;
  refreshing: boolean;
}>();
const emit = defineEmits<{ play: []; follow: []; refresh: []; profile: [id: string] }>();
</script>
<template>
  <header class="catalog-header artist-header" :aria-busy="refreshing">
    <img
      v-if="artist.coverUrl"
      class="catalog-cover artist"
      :src="artist.coverUrl"
      alt=""
      referrerpolicy="no-referrer"
      decoding="async"
    />
    <div v-else class="catalog-cover artist catalog-placeholder"><Icon name="user" /></div>
    <div class="catalog-heading">
      <h1>{{ artist.name }}</h1>
      <p v-if="artist.aliases.length || profileAccountId" class="artist-alias muted">
        <span v-if="artist.aliases.length">{{ artist.aliases.join(' / ') }}</span>
        <button
          v-if="profileAccountId"
          class="text-button artist-profile-link"
          type="button"
          @click="emit('profile', profileAccountId)"
        >
          个人页
        </button>
      </p>
      <div class="catalog-actions artist-actions">
        <button
          class="primary"
          :disabled="playDisabled"
          data-tooltip="播放当前热门歌曲列表"
          @click="emit('play')"
        >
          <Icon name="play" />播放全部
        </button>
        <button
          v-if="!isSelf"
          :disabled="followDisabled"
          :aria-pressed="followed ?? undefined"
          @click="emit('follow')"
        >
          <Icon :name="followed ? 'check' : 'plus'" />{{
            followed == null ? '关注状态未知' : followed ? '已关注' : '关注'
          }}
        </button>
        <button
          class="icon-button"
          aria-label="刷新详情"
          :class="{ 'catalog-refreshing': refreshing }"
          :aria-busy="refreshing"
          :disabled="refreshDisabled"
          @click="emit('refresh')"
        >
          <Icon name="refresh" />
        </button>
      </div>
    </div>
  </header>
</template>
<style scoped>
.artist-profile-link {
  margin-inline-start: 8px;
}
.artist-profile-link:first-child {
  margin-inline-start: 0;
}
</style>
