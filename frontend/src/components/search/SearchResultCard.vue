<script setup lang="ts">
import { computed } from 'vue';
import type { SearchItem } from '../../services/search';
import { artworkAt } from '../../services/artwork';
import Icon from '../Icon.vue';
const props = defineProps<{ item: SearchItem }>();
const emit = defineEmits<{ open: [item: SearchItem] }>();
const unavailable = computed(() =>
  props.item.kind === 'voice'
    ? !props.item.voice.radioId
    : props.item.kind === 'mv'
      ? !props.item.mv.mvId
      : false,
);
const card = computed(() => {
  const i = props.item;
  switch (i.kind) {
    case 'artist':
      return {
        name: i.artist.name,
        cover: i.artist.coverUrl,
        sub: i.artist.aliases.join(' / ') || `${i.artist.songCount} 首歌曲`,
        round: true,
      };
    case 'user':
      return {
        name: i.user.name,
        cover: i.user.avatarUrl,
        sub: i.user.signature || '暂无简介',
        round: true,
      };
    case 'playlist':
      return {
        name: i.playlist.name,
        cover: i.playlist.coverUrl,
        sub: `${i.playlist.trackCount} 首 · ${i.playlist.creator}`,
        round: false,
      };
    case 'album':
      return {
        name: i.album.name,
        cover: i.album.coverUrl,
        sub: i.album.artists.map((a) => a.name).join(' / '),
        round: false,
      };
    case 'podcast':
      return {
        name: i.podcast.name,
        cover: i.podcast.coverUrl,
        sub: `${i.podcast.programCount} 期 · ${i.podcast.creator}`,
        round: false,
      };
    case 'voice':
      return {
        name: i.voice.name,
        cover: i.voice.coverUrl,
        sub: i.voice.radioName || i.voice.creator,
        round: false,
      };
    case 'mv':
      return {
        name: i.mv.title,
        cover: i.mv.coverUrl,
        sub: i.mv.artists.map((a) => a.name).join(' / '),
        round: false,
      };
    case 'related':
      return { name: i.keyword, cover: null, sub: '', round: false };
    case 'song':
      return {
        name: i.track.name,
        cover: i.track.coverUrl,
        sub: i.track.artists.join(' / '),
        round: false,
      };
  }
});
</script>
<template>
  <button
    class="search-result-card"
    :disabled="unavailable"
    :class="[
      {
        'search-person-card': card.round,
        'search-related-word': item.kind === 'related',
        'search-video-card': item.kind === 'mv',
        'search-cover-card': ['playlist', 'album', 'podcast', 'mv'].includes(item.kind),
        'search-voice-card': item.kind === 'voice',
      },
    ]"
    @click="emit('open', item)"
    :aria-label="`${item.kind === 'voice' ? '查看所属播客' : item.kind === 'user' ? '查看用户信息' : '查看'} ${card.name}`"
  >
    <span v-if="item.kind !== 'related'" class="search-result-art"
      ><img
        v-if="card.cover"
        :src="artworkAt(card.cover, 320)"
        alt=""
        loading="lazy"
        decoding="async"
        referrerpolicy="no-referrer" /><Icon v-else :name="card.round ? 'user' : 'music'"
    /></span>
    <span class="search-result-name" :data-tooltip="card.name">{{ card.name }}</span>
    <span v-if="unavailable" class="search-result-sub">暂不可打开</span>
    <span v-else-if="card.sub" class="search-result-sub" :data-tooltip="card.sub">{{
      card.sub
    }}</span>
  </button>
</template>
