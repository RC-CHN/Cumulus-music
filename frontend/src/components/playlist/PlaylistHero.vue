<script setup lang="ts">
import type { PlaylistView } from '../../services/contracts';
import type { MenuItem } from '../../services/menu';
import ActionMenu from '../ActionMenu.vue';
import Icon from '../Icon.vue';
import PlaylistDescription from './PlaylistDescription.vue';

defineProps<{
  playlist: PlaylistView;
  total: number;
  playable: boolean;
  preparing: boolean;
  downloadDisabled: boolean;
  subscribed: boolean;
  busy: boolean;
  items: MenuItem[];
}>();
defineEmits<{
  edit: [];
  play: [];
  download: [];
  subscribe: [];
  action: [key: string];
}>();
</script>

<template>
  <section class="playlist-hero" aria-label="歌单详情">
    <div class="playlist-art">
      <img
        v-if="playlist.coverUrl"
        :src="playlist.coverUrl"
        alt="歌单封面"
        referrerpolicy="no-referrer"
      />
      <template v-else><Icon name="music" /><span class="sr-only">暂无歌单封面</span></template>
      <span v-if="playlist.private" class="playlist-privacy"><Icon name="lock" />私密</span>
    </div>
    <div class="hero-copy">
      <div class="playlist-title">
        <h1 :title="playlist.name">{{ playlist.name }}</h1>
        <button
          v-if="playlist.owned && !playlist.favorite"
          class="icon-button playlist-edit"
          title="编辑歌单"
          aria-label="编辑歌单"
          :disabled="busy"
          @click="$emit('edit')"
        >
          <Icon name="edit" /><span class="sr-only">编辑歌单</span>
        </button>
      </div>
      <div class="playlist-summary">
        <PlaylistDescription :text="playlist.description" />
        <div class="playlist-credits">
          <span class="ellipsis" :title="playlist.creator">{{ playlist.creator }}</span>
          <span class="playlist-total">{{ total }} 首歌曲</span>
        </div>
        <div class="playlist-actions" :aria-busy="preparing">
          <button class="primary" :disabled="!playable || preparing" @click="$emit('play')">
            <Icon name="play" />{{ preparing ? '准备中…' : '播放全部' }}
          </button>
          <button
            v-if="!playlist.owned"
            class="download-button playlist-subscribe"
            :aria-label="subscribed ? '取消收藏歌单' : '收藏歌单'"
            :title="subscribed ? '取消收藏歌单' : '收藏歌单'"
            :aria-pressed="subscribed"
            :disabled="busy"
            @click="$emit('subscribe')"
          >
            <Icon :name="subscribed ? 'check' : 'star'" />{{ subscribed ? '已收藏' : '收藏' }}
          </button>
          <button
            class="download-button"
            aria-label="下载当前歌曲列表"
            title="下载当前歌曲列表"
            :disabled="downloadDisabled"
            @click="$emit('download')"
          >
            <Icon name="download" />下载
          </button>
          <ActionMenu
            v-if="items.length"
            label="更多歌单操作"
            :items="items"
            @select="$emit('action', $event)"
          />
        </div>
      </div>
    </div>
  </section>
</template>
