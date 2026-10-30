<script setup lang="ts">
import { ref, watch } from 'vue';
import type { PlaylistView } from '../../services/contracts';
import Icon from '../Icon.vue';
const props = defineProps<{ playlist: PlaylistView; selected: boolean; playing: boolean }>();
const broken = ref(false);
watch(
  () => props.playlist.coverUrl,
  () => {
    broken.value = false;
  },
);
</script>
<template>
  <span class="sidebar-playlist-background">
    <span class="sidebar-playlist-cover">
      <img
        v-if="playlist.coverUrl && !broken"
        :src="playlist.coverUrl"
        alt=""
        loading="lazy"
        decoding="async"
        referrerpolicy="no-referrer"
        @error="broken = true"
      />
      <Icon v-else :name="playlist.private ? 'lock' : 'music'" />
      <span v-if="playlist.private && playlist.coverUrl && !broken" class="sidebar-private"
        ><Icon name="lock"
      /></span>
      <span v-if="playing" class="sidebar-playing" aria-hidden="true"><i /><i /><i /></span>
    </span>
    <span class="sidebar-playlist-name">{{ playlist.name }}</span>
  </span>
</template>
