<script setup lang="ts">
import { nextTick, onMounted, ref } from 'vue';
import type { usePlaylistCreator } from '../../composables/usePlaylistCreator';
import Icon from '../Icon.vue';
import PlaylistCard from '../PlaylistCard.vue';

const props = defineProps<{ model: ReturnType<typeof usePlaylistCreator> }>();
const emit = defineEmits<{ open: [id: string] }>();
const root = ref<HTMLElement>();
const failedAvatar = ref(false);
onMounted(async () => {
  const origin = document.activeElement;
  await nextTick();
  if (!root.value?.isConnected || root.value.closest('[inert]')) return;
  if (document.activeElement !== document.body && document.activeElement !== origin) return;
  const card = Array.from(
    root.value.querySelectorAll<HTMLButtonElement>('[data-library-open]'),
  ).find((node) => node.dataset.libraryOpen === props.model.opened.value);
  (card ?? root.value.querySelector('h1'))?.focus({ preventScroll: true });
});
function open(id: string) {
  props.model.opened.value = id;
  emit('open', id);
}
</script>

<template>
  <section
    ref="root"
    class="playlist-creator-page"
    aria-label="用户公开主页"
    :aria-busy="model.busy.value"
  >
    <header class="creator-page-header">
      <span v-if="model.profile.value" class="creator-page-portrait">
        <img
          v-if="model.profile.value.avatarUrl && !failedAvatar"
          :src="model.profile.value.avatarUrl"
          alt=""
          referrerpolicy="no-referrer"
          @error="failedAvatar = true"
        />
        <Icon v-else name="user" />
      </span>
      <div>
        <h1 tabindex="-1">{{ model.profile.value?.nickname ?? '用户主页' }}</h1>
        <p v-if="model.profile.value?.signature" class="muted">
          {{ model.profile.value.signature }}
        </p>
      </div>
    </header>
    <p v-if="model.busy.value" role="status">正在读取公开资料…</p>
    <p v-else-if="model.failed.value" role="alert">
      无法读取公开资料
      <button class="text-button" type="button" @click="model.load">重试</button>
    </p>
    <p v-else-if="model.profile.value === null" class="muted">用户不存在</p>
    <template v-else-if="model.profile.value">
      <div class="creator-page-groups" role="group" aria-label="公开歌单">
        <button
          class="text-button"
          type="button"
          :aria-pressed="model.group.value === 'created'"
          @click="model.select('created')"
        >
          创建的歌单 {{ model.profile.value.created.length }}
        </button>
        <button
          class="text-button"
          type="button"
          :aria-pressed="model.group.value === 'collected'"
          @click="model.select('collected')"
        >
          收藏的歌单 {{ model.profile.value.collected.length }}
        </button>
      </div>
      <div v-if="model.cards.value.length" class="playlist-grid">
        <PlaylistCard
          v-for="list in model.cards.value"
          :key="list.id"
          :playlist="list"
          :items="[]"
          open-only
          @open="open(list.id)"
        />
      </div>
      <p v-else class="muted">暂无公开歌单</p>
      <nav v-if="model.pages.value > 1" aria-label="公开歌单分页">
        <button
          class="text-button"
          type="button"
          :disabled="model.page.value === 0"
          @click="model.page.value--"
        >
          上一页
        </button>
        <button
          class="text-button"
          type="button"
          :disabled="model.page.value + 1 >= model.pages.value"
          @click="model.page.value++"
        >
          下一页
        </button>
      </nav>
    </template>
  </section>
</template>

<style scoped>
.creator-page-header {
  display: flex;
  gap: 20px;
  align-items: center;
  overflow-wrap: anywhere;
  margin-bottom: 20px;
}
.creator-page-header h1 {
  margin: 0;
}
.creator-page-portrait {
  display: grid;
  flex: 0 0 80px;
  height: 80px;
  place-items: center;
  overflow: hidden;
  border-radius: 50%;
  background: var(--surface-alt);
}
.creator-page-portrait img {
  width: 100%;
  height: 100%;
  object-fit: cover;
}
.creator-page-groups {
  display: flex;
  gap: 12px;
  margin-bottom: 12px;
}
.creator-page-groups [aria-pressed='true'] {
  font-weight: 600;
  background: var(--surface-alt);
}
</style>
