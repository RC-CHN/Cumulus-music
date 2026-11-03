<script setup lang="ts">
import Icon from '../Icon.vue';
import type { usePlaylistSubscribers } from '../../composables/usePlaylistSubscribers';

defineProps<{ model: ReturnType<typeof usePlaylistSubscribers> }>();
</script>

<template>
  <section class="playlist-subscribers" aria-label="歌单收藏者">
    <p v-if="model.busy.value" role="status">正在读取收藏者…</p>
    <p v-if="model.failed.value" role="alert">
      无法读取收藏者
      <button class="text-button" type="button" @click="model.retry">重试</button>
    </p>
    <template v-if="model.value.value">
      <p v-if="!model.value.value.subscribers.length" class="muted">暂无收藏者</p>
      <ul v-else class="playlist-subscriber-grid">
        <li v-for="(person, index) in model.value.value.subscribers" :key="index">
          <div class="playlist-subscriber-avatar">
            <img
              v-if="person.avatarUrl"
              :src="person.avatarUrl"
              alt=""
              referrerpolicy="no-referrer"
            />
            <Icon v-else name="user" />
          </div>
          <strong>{{ person.nickname }}</strong>
          <span class="muted">{{ person.signature || '这个人很懒，什么也没留下' }}</span>
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
