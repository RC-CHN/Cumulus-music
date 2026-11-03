<script setup lang="ts">
import { onMounted, ref } from 'vue';
import Icon from '../Icon.vue';
import type { usePlaylistSubscribers } from '../../composables/usePlaylistSubscribers';

type CardFocus = { row: number; scrollTop: number };
const props = defineProps<{
  model: ReturnType<typeof usePlaylistSubscribers>;
  restoreFocus?: CardFocus;
}>();
const emit = defineEmits<{
  profile: [id: string, focus: CardFocus];
  restored: [];
}>();
const root = ref<HTMLElement>();

function openProfile(id: string | null, row: number) {
  if (!id || props.model.busy.value) return;
  emit('profile', id, { row, scrollTop: root.value?.scrollTop ?? 0 });
}

onMounted(() => {
  const focus = props.restoreFocus;
  if (!focus) return;
  if (root.value) root.value.scrollTop = focus.scrollTop;
  root.value?.querySelector<HTMLElement>(`[data-subscriber-row="${focus.row}"]`)?.focus({
    preventScroll: true,
  });
  emit('restored');
});
</script>

<template>
  <section ref="root" class="playlist-subscribers" aria-label="歌单收藏者">
    <p v-if="model.busy.value" role="status">正在读取收藏者…</p>
    <p v-if="model.failed.value" role="alert">
      无法读取收藏者
      <button class="text-button" type="button" @click="model.retry">重试</button>
    </p>
    <template v-if="model.value.value">
      <p v-if="!model.value.value.subscribers.length" class="muted">暂无收藏者</p>
      <ul v-else class="playlist-subscriber-grid">
        <li v-for="(person, index) in model.value.value.subscribers" :key="index">
          <component
            :is="person.userId ? 'button' : 'div'"
            class="playlist-subscriber-card"
            :type="person.userId ? 'button' : undefined"
            :aria-label="person.userId ? `查看${person.nickname}的主页` : undefined"
            :disabled="person.userId ? model.busy.value : undefined"
            :data-subscriber-row="person.userId ? index : undefined"
            @click="openProfile(person.userId, index)"
          >
            <span class="playlist-subscriber-avatar">
              <img
                v-if="person.avatarUrl"
                :src="person.avatarUrl"
                alt=""
                referrerpolicy="no-referrer"
              />
              <Icon v-else name="user" />
            </span>
            <strong>{{ person.nickname }}</strong>
            <span class="muted">{{ person.signature || '这个人很懒，什么也没留下' }}</span>
          </component>
        </li>
      </ul>
      <nav
        v-if="model.hasPrevious.value || model.value.value.nextOffset != null"
        aria-label="收藏者分页"
      >
        <button
          class="text-button"
          type="button"
          :disabled="!model.hasPrevious.value || model.busy.value"
          @click="model.previous"
        >
          上一页
        </button>
        <button
          class="text-button"
          type="button"
          :disabled="model.value.value.nextOffset == null || model.busy.value"
          @click="model.next"
        >
          下一页
        </button>
      </nav>
    </template>
  </section>
</template>
