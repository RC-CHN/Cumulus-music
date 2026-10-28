<script setup lang="ts">
import { ref } from 'vue';
import type { PlaylistView } from '../services/contracts';
import type { MenuItem } from '../services/menu';
import Icon from './Icon.vue';
import ActionMenu from './ActionMenu.vue';
defineProps<{ playlist: PlaylistView; items: MenuItem[] }>();
defineEmits<{ open: []; action: [key: string] }>();
const menu = ref<InstanceType<typeof ActionMenu>>();
</script>
<template>
  <article class="playlist-shell" @contextmenu.prevent="items.length && menu?.show()">
    <button class="playlist-card" :title="playlist.name" @click="$emit('open')">
      <div class="card-cover">
        <img
          v-if="playlist.coverUrl"
          :src="playlist.coverUrl"
          alt=""
          loading="lazy"
          decoding="async"
          referrerpolicy="no-referrer"
        />
        <Icon v-else name="music" />
        <span class="card-count">{{ playlist.trackCount }} 首</span>
      </div>
      <span class="card-title">{{ playlist.name }}</span>
      <small>{{ playlist.owned ? '我创建的' : '已收藏' }}</small>
    </button>
    <ActionMenu
      v-if="items.length"
      ref="menu"
      class="playlist-card-menu"
      :label="`管理歌单：${playlist.name}`"
      :items="items"
      @select="$emit('action', $event)"
    />
  </article>
</template>
