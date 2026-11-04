<script setup lang="ts">
import { nextTick, onMounted, ref, watch } from 'vue';
import type { useCommunity } from '../../composables/useCommunity';
import type { CommunityResource } from '../../services/community';
import CommunityEventCard from '../community/CommunityEventCard.vue';

const props = defineProps<{ model: ReturnType<typeof useCommunity>; playable: boolean }>();
const emit = defineEmits<{
  open: [id: string];
  resource: [value: CommunityResource];
  topic: [name: string];
}>();
const { visible, group, groups, continuation, loaded, pending, feedError, unavailable, autoMore } =
  props.model;
const section = ref<HTMLElement>();
const heading = ref<HTMLElement>();
const sentinel = ref<HTMLElement>();

onMounted(async () => {
  await nextTick();
  const saved = props.model.opener.value;
  if (!saved || !section.value) return;
  const opener = [
    ...section.value.querySelectorAll<HTMLButtonElement>(
      '[data-community-opener], [data-topic-opener]',
    ),
  ].find((node) => node.dataset.communityOpener === saved || node.dataset.topicOpener === saved);
  if (opener) {
    opener.scrollIntoView({ block: 'nearest', behavior: 'instant' });
    opener.focus({ preventScroll: true });
    props.model.opener.value = null;
  }
});
watch(
  sentinel,
  (node, _, cleanup) => {
    if (!node) return;
    const observer = new IntersectionObserver(
      ([entry]) => {
        if (entry?.isIntersecting && autoMore.value && !feedError.value) void props.model.next();
      },
      { rootMargin: '0px 0px 60px 0px' },
    );
    observer.observe(node);
    cleanup(() => observer.disconnect());
  },
  { flush: 'post' },
);

async function choose(page: number) {
  props.model.choose(page);
  await nextTick();
  heading.value?.scrollIntoView({ block: 'nearest', behavior: 'instant' });
  heading.value?.focus({ preventScroll: true });
}
async function more() {
  const before = groups.value;
  await props.model.next();
  if (groups.value > before) await choose(before);
}
</script>

<template>
  <section ref="section" class="profile-notes" aria-label="用户笔记" :aria-busy="pending">
    <h2 ref="heading" class="sr-only" tabindex="-1">用户笔记</h2>
    <p v-if="pending && !loaded" role="status">正在读取笔记…</p>
    <p v-else-if="unavailable" role="status">此用户的笔记不可查看</p>
    <p v-else-if="loaded && !visible.length" class="muted">暂无笔记</p>
    <div v-if="visible.length" class="profile-note-list">
      <CommunityEventCard
        v-for="item in visible"
        :key="item.key"
        :card="item"
        :playable="playable"
        @open="emit('open', $event)"
        @topic="
          (name, opener) => {
            model.opener.value = opener;
            emit('topic', name);
          }
        "
        @resource="emit('resource', $event)"
      />
    </div>
    <div v-if="continuation && autoMore && !feedError" ref="sentinel" aria-hidden="true" />
    <p v-if="feedError" role="alert">
      {{ feedError }}
      <button class="text-button" type="button" @click="model.retry">重试</button>
    </p>
    <nav v-if="groups > 1 || continuation" class="profile-note-pagination" aria-label="笔记分页">
      <button v-if="group > 0" class="text-button" type="button" @click="choose(group - 1)">
        上一页
      </button>
      <button
        v-if="group + 1 < groups"
        class="text-button"
        type="button"
        @click="choose(group + 1)"
      >
        下一页
      </button>
      <button
        v-else-if="continuation && !feedError"
        class="text-button"
        type="button"
        :disabled="pending"
        @click="more"
      >
        {{ pending ? '正在读取…' : '更多笔记' }}
      </button>
    </nav>
  </section>
</template>

<style scoped>
.profile-notes {
  max-width: 760px;
}
.profile-note-list {
  display: grid;
  gap: 14px;
}
.profile-note-pagination {
  display: flex;
  gap: 1rem;
  margin-block: 1rem;
}
</style>
