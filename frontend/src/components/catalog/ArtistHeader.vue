<script setup lang="ts">
import type { ArtistCard } from '../../services/catalog';
import Icon from '../Icon.vue';
defineProps<{
  artist: ArtistCard;
  followed: boolean | null;
  followDisabled: boolean;
  playDisabled: boolean;
  refreshDisabled: boolean;
  refreshing: boolean;
}>();
const emit = defineEmits<{ play: []; follow: []; refresh: [] }>();
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
      <p v-if="artist.aliases.length" class="artist-alias muted">
        {{ artist.aliases.join(' / ') }}
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
