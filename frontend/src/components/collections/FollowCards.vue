<script setup lang="ts">
import { computed, onUnmounted, ref, watch } from 'vue';
import type { FollowCard } from '../../services/collections';
import { useVirtualRows } from '../../composables/useVirtualRows';
import Icon from '../Icon.vue';
const props = defineProps<{ entries: FollowCard[] }>();
const emit = defineEmits<{ artist: [id: string] }>();
const columns = ref(2);
const { container, start, end, onScroll } = useVirtualRows(
  () => Math.ceil(props.entries.length / columns.value),
  96,
  2,
);
const visible = computed(() =>
  props.entries.slice(start.value * columns.value, end.value * columns.value),
);
let observer: ResizeObserver | undefined;
watch(
  container,
  (element) => {
    observer?.disconnect();
    if (element) {
      observer = new ResizeObserver(([entry]) => {
        columns.value = (entry?.contentRect.width ?? 0) >= 650 ? 2 : 1;
      });
      observer.observe(element);
    }
  },
  { flush: 'post' },
);
onUnmounted(() => observer?.disconnect());
</script>
<template>
  <div ref="container" class="collection-follow-scroll" @scroll.passive="onScroll">
    <div :style="{ height: `${start * 96}px` }" aria-hidden="true" />
    <div
      class="collection-follow-grid"
      :style="{ gridTemplateColumns: `repeat(${columns},minmax(0,1fr))` }"
    >
      <component
        :is="entry.artist ? 'button' : 'article'"
        v-for="(entry, index) in visible"
        :key="start * columns + index"
        class="collection-follow-card"
        :aria-label="entry.artist ? `查看歌手 ${entry.artist.name}` : undefined"
        @click="entry.artist && emit('artist', entry.artist.id)"
      >
        <img
          v-if="entry.user?.avatarUrl || entry.artist?.coverUrl"
          :src="entry.user?.avatarUrl ?? entry.artist?.coverUrl ?? undefined"
          alt=""
          loading="lazy"
          decoding="async"
          referrerpolicy="no-referrer"
        />
        <span v-else class="collection-avatar"><Icon name="user" /></span>
        <span class="collection-follow-info"
          ><strong>{{ entry.artist?.name || entry.user?.name || '资料暂缺' }}</strong>
          <small v-if="entry.user?.unavailable">关联用户已注销</small>
          <small v-else-if="entry.user?.signature">{{ entry.user.signature }}</small>
          <small v-if="!entry.artist && entry.user">用户主页暂未接入</small>
        </span>
      </component>
    </div>
    <div
      :style="{ height: `${(Math.ceil(entries.length / columns) - end) * 96}px` }"
      aria-hidden="true"
    />
  </div>
</template>
