<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue';
import type {
  TopicGroup,
  TopicVisit,
  useCommunityTopic,
} from '../../composables/useCommunityTopic';
import type { CommunityResource } from '../../services/community';
import CommunityEventCard from './CommunityEventCard.vue';
import Icon from '../Icon.vue';

const props = defineProps<{
  model: ReturnType<typeof useCommunityTopic>;
  state: TopicVisit;
  playable: boolean;
}>();
const emit = defineEmits<{
  detail: [handle: string];
  topic: [name: string];
  resource: [value: CommunityResource];
}>();
const PAGE = 20;
const surface = ref<HTMLElement>();
const allHeading = ref<HTMLElement>();
const moreSentinel = ref<HTMLElement>();
const labels = { hot: '热门笔记', all: '全部' } as const;
const groups = ['hot', 'all'] as const;
const pages = computed(() => Math.max(1, Math.ceil(props.state.all.cards.length / PAGE)));
const number = new Intl.NumberFormat('zh-CN', { notation: 'compact' });
function setAllHeading(node: unknown) {
  allHeading.value = node instanceof HTMLElement ? node : undefined;
}
function setMoreSentinel(node: unknown) {
  moreSentinel.value = node instanceof HTMLElement ? node : undefined;
}
function cards(group: TopicGroup) {
  return group === 'hot'
    ? props.state.hot.cards
    : props.state.all.cards.slice(props.state.group * PAGE, (props.state.group + 1) * PAGE);
}
function remember(group: TopicGroup, key: string) {
  props.state.opener = { group, key };
  if (surface.value) props.state.scrollTop = surface.value.scrollTop;
}
async function restore() {
  await nextTick();
  if (!surface.value || !props.state.meta) return;
  surface.value.scrollTop = props.state.scrollTop;
  const marker = props.state.opener;
  if (!marker) return;
  const target = Array.from(
    surface.value.querySelectorAll<HTMLButtonElement>(
      `[data-topic-group="${marker.group}"] [data-topic-opener], [data-topic-group="${marker.group}"] [data-community-opener]`,
    ),
  ).find(
    (button) =>
      button.dataset.topicOpener === marker.key || button.dataset.communityOpener === marker.key,
  );
  if (target) {
    target.focus({ preventScroll: true });
    props.state.opener = null;
  }
}
let timer: ReturnType<typeof setTimeout> | undefined;
let polling = false;
let mounted = false;
let foregroundRevision = 0;
const foreground = () => mounted && document.visibilityState === 'visible' && document.hasFocus();
function scheduleNotice() {
  if (timer !== undefined) clearTimeout(timer);
  timer = undefined;
  if (!foreground() || polling || !props.state.meta || props.state.unavailable) return;
  timer = setTimeout(async () => {
    timer = undefined;
    if (!foreground()) return;
    polling = true;
    const revision = foregroundRevision;
    try {
      await props.model.notice(props.state, () => revision === foregroundRevision && foreground());
    } finally {
      polling = false;
      scheduleNotice();
    }
  }, 20_000);
}
function foregroundChanged() {
  foregroundRevision++;
  scheduleNotice();
}
onMounted(() => {
  mounted = true;
  document.addEventListener('visibilitychange', foregroundChanged);
  window.addEventListener('focus', foregroundChanged);
  window.addEventListener('blur', foregroundChanged);
  scheduleNotice();
});
onUnmounted(() => {
  mounted = false;
  document.removeEventListener('visibilitychange', foregroundChanged);
  window.removeEventListener('focus', foregroundChanged);
  window.removeEventListener('blur', foregroundChanged);
  scheduleNotice();
});
watch([() => props.state.meta, () => props.state.unavailable], scheduleNotice);
onMounted(restore);
watch(() => props.state.meta, restore);
watch(
  moreSentinel,
  (target, _, cleanup) => {
    if (!target) return;
    const observer = new IntersectionObserver(
      ([entry]) => {
        if (
          entry?.isIntersecting &&
          props.state.group + 1 === pages.value &&
          props.state.all.cards.length > 0
        )
          void props.model.next();
      },
      { root: surface.value, rootMargin: '0px 0px 60px 0px' },
    );
    observer.observe(target);
    cleanup(() => observer.disconnect());
  },
  { flush: 'post' },
);

async function choose(page: number) {
  props.state.group = page;
  await nextTick();
  allHeading.value?.scrollIntoView({ block: 'start', behavior: 'instant' });
  allHeading.value?.focus({ preventScroll: true });
}
async function more() {
  const before = pages.value;
  await props.model.next();
  if (pages.value > before) await choose(before);
}
</script>

<template>
  <section ref="surface" class="community-topic-page" aria-label="话题">
    <header class="page-heading community-topic-heading">
      <h1>话题</h1>
      <button
        v-if="state.hasNew && !state.unavailable"
        type="button"
        class="text-button"
        :disabled="state.refreshing || state.all.pending || state.hot.pending"
        @click="model.refresh()"
      >
        有新笔记，点击查看
      </button>
      <button
        v-if="state.meta && !state.unavailable"
        class="icon-button"
        type="button"
        aria-label="刷新话题"
        :disabled="state.refreshing || state.hot.pending || state.all.pending"
        @click="model.refresh()"
      >
        <Icon name="refresh" />
      </button>
    </header>
    <p v-if="state.pending && !state.meta" role="status">正在读取话题…</p>
    <p v-else-if="state.unavailable" role="status">此话题不存在或无法查看。</p>
    <p v-else-if="state.error" role="alert">
      {{ state.error }}
      <button type="button" class="text-button" @click="model.retry()">重试</button>
    </p>
    <template v-else-if="state.meta">
      <div class="community-topic-banner">
        <img
          v-if="state.meta.wideCoverUrl"
          class="wide"
          :src="state.meta.wideCoverUrl"
          alt=""
          referrerpolicy="no-referrer"
        />
        <img
          v-if="state.meta.coverUrl"
          class="cover"
          :src="state.meta.coverUrl"
          alt=""
          referrerpolicy="no-referrer"
        />
        <div class="community-topic-banner-copy">
          <h2>{{ state.meta.title }}</h2>
          <p v-if="state.meta.participants !== null">
            {{ number.format(state.meta.participants) }} 人参与
          </p>
        </div>
      </div>
      <p v-if="state.meta.description" class="muted community-topic-description">
        {{ state.meta.description }}
      </p>
      <template v-for="group in groups" :key="group">
        <section
          v-if="group === 'all' || state.hot.cards.length || state.hot.error || state.hot.pending"
          :aria-label="labels[group]"
          :data-topic-group="group"
          class="community-topic-group"
        >
          <h2 v-if="group === 'all'" :ref="setAllHeading" tabindex="-1">全部</h2>
          <h2 v-else>热门笔记</h2>
          <p v-if="state[group].pending && !state[group].loaded" role="status">正在读取笔记…</p>
          <p v-if="state[group].error" role="alert">
            {{ state[group].error }}
            <button class="text-button" type="button" @click="model.retry(group)">重试</button>
          </p>
          <p
            v-else-if="group === 'all' && state.all.loaded && !state.all.cards.length"
            class="muted"
          >
            暂无笔记
          </p>
          <div class="community-topic-cards">
            <CommunityEventCard
              v-for="card in cards(group)"
              :key="card.key"
              :card="card"
              :playable="playable"
              @open="
                (key) => {
                  remember(group, key);
                  emit('detail', key);
                }
              "
              @topic="
                (name, opener) => {
                  remember(group, opener);
                  emit('topic', name);
                }
              "
              @resource="(value) => emit('resource', value)"
            />
          </div>
          <template v-if="group === 'all'">
            <div
              v-if="
                state.continuation &&
                state.group + 1 === pages &&
                !state.all.error &&
                !state.all.pending
              "
              :ref="setMoreSentinel"
              class="community-topic-sentinel"
              aria-hidden="true"
            />
            <nav
              v-if="pages > 1 || state.continuation"
              class="community-topic-pagination"
              aria-label="话题笔记分页"
            >
              <button
                v-if="state.group > 0"
                class="text-button"
                type="button"
                @click="choose(state.group - 1)"
              >
                上一页
              </button>
              <button
                v-if="state.group + 1 < pages"
                class="text-button"
                type="button"
                @click="choose(state.group + 1)"
              >
                下一页
              </button>
              <button
                v-else-if="state.continuation && !state.all.error"
                class="text-button"
                type="button"
                :disabled="state.all.pending || state.refreshing"
                @click="more"
              >
                更多笔记
              </button>
            </nav>
          </template>
        </section>
      </template>
    </template>
  </section>
</template>

<style scoped>
.community-topic-page {
  overflow: auto;
  height: 100%;
  padding: 20px clamp(16px, 4vw, 52px) 40px;
}
.community-topic-heading {
  display: flex;
  justify-content: space-between;
  align-items: center;
}
.community-topic-banner {
  min-height: 170px;
  position: relative;
  display: grid;
  place-items: center;
  overflow: hidden;
  border-radius: 10px;
  background: var(--surface-alt);
}
.community-topic-banner img {
  position: absolute;
  width: 100%;
  height: 100%;
  object-fit: cover;
  opacity: 0.45;
}
.community-topic-banner .cover {
  max-width: 526px;
  mask-image: linear-gradient(90deg, transparent, black 14%, black 86%, transparent);
}
.community-topic-banner-copy {
  position: relative;
  padding: 20px;
  text-align: center;
  overflow-wrap: anywhere;
}
.community-topic-description {
  white-space: pre-wrap;
  overflow-wrap: anywhere;
  padding-block: 12px;
  border-bottom: 1px solid var(--line);
}
.community-topic-group {
  margin-top: 22px;
}
.community-topic-group h2 {
  font-size: 18px;
}
.community-topic-cards {
  max-width: 850px;
  display: grid;
  gap: 14px;
}
.community-topic-pagination {
  display: flex;
  gap: 1rem;
  margin-top: 12px;
}
.community-topic-sentinel {
  height: 1px;
}
</style>
