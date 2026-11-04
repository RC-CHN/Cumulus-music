<script setup lang="ts">
import { nextTick, onMounted, ref, watch } from 'vue';
import type { useCommunity } from '../../composables/useCommunity';
import type { useCommunityNotice } from '../../composables/useCommunityNotice';
import type { CommunityResource } from '../../services/community';
import CommunityComposer from './CommunityComposer.vue';
import CommunityEventCard from './CommunityEventCard.vue';
import CommunityNewSongNotice from './CommunityNewSongNotice.vue';
import Icon from '../Icon.vue';

const props = defineProps<{
  model: ReturnType<typeof useCommunity>;
  notice: ReturnType<typeof useCommunityNotice>;
  detailKey?: string;
  accountEpoch: string;
  playable: boolean;
}>();
const emit = defineEmits<{
  open: [id: string];
  resource: [value: CommunityResource];
  back: [];
  openNewSongs: [];
  topic: [name: string];
}>();
const model = props.model;
const {
  visible,
  group,
  groups,
  continuation,
  loaded,
  pending,
  feedError,
  autoMore,
  detail,
  detailPending,
  detailError,
  latest,
  hot,
  total,
  nextOffset,
  commentsPending,
  commentsError,
} = model;
const surface = ref<HTMLElement>();
const heading = ref<HTMLElement>();
const sentinel = ref<HTMLElement>();
const offsetHistory = ref([0]);
const lastCommentAttempt = ref(0);
const date = new Intl.DateTimeFormat('zh-CN', { dateStyle: 'medium' });
const offset = () => offsetHistory.value.at(-1) ?? 0;

onMounted(restore);
async function restore() {
  await nextTick();
  const root = surface.value;
  if (!root) return;
  if (props.detailKey) {
    const marker = model.detailTopicOpeners.get(props.detailKey);
    if (!detail.value || !marker) return;
    const target = [...root.querySelectorAll<HTMLButtonElement>('[data-topic-opener]')].find(
      (button) => button.dataset.topicOpener === marker,
    );
    if (!target) return;
    target.scrollIntoView({ block: 'nearest', behavior: 'instant' });
    target.focus({ preventScroll: true });
    model.detailTopicOpeners.delete(props.detailKey);
    return;
  }
  root.scrollTop = model.scrollTop.value;
  if (!model.opener.value) return;
  const key = model.opener.value;
  const target = [
    ...root.querySelectorAll<HTMLButtonElement>('[data-community-opener], [data-topic-opener]'),
  ].find((button) => button.dataset.communityOpener === key || button.dataset.topicOpener === key);
  if (!target) return;
  target.focus({ preventScroll: true });
  model.opener.value = null;
}
watch(detail, () => {
  if (props.detailKey) void restore();
});
function openTopic(name: string, opener: string) {
  if (props.detailKey) model.rememberDetailTopic(props.detailKey, opener);
  else {
    rememberScroll();
    model.opener.value = opener;
  }
  emit('topic', name);
}
defineExpose({ restore, refresh });

watch(
  sentinel,
  (element, _, cleanup) => {
    if (!element) return;
    const observer = new IntersectionObserver(
      ([entry]) => {
        if (entry?.isIntersecting && autoMore.value && !feedError.value) void model.next();
      },
      { root: surface.value ?? null, rootMargin: '0px 0px 70px 0px' },
    );
    observer.observe(element);
    cleanup(() => observer.disconnect());
  },
  { flush: 'post' },
);
function open(id: string) {
  model.opener.value = id;
  emit('open', id);
}
function openPublished(detail: string) {
  model.opener.value = 'composer';
  emit('open', detail);
}
function openNewSongs() {
  model.opener.value = 'new-song-notice';
  emit('openNewSongs');
}
function rememberScroll() {
  if (!props.detailKey && surface.value) model.scrollTop.value = surface.value.scrollTop;
}
function resetView() {
  surface.value?.scrollTo({ top: 0, behavior: 'instant' });
  heading.value?.focus({ preventScroll: true });
}
function refresh() {
  surface.value?.scrollTo({ top: 0, behavior: 'instant' });
  void model.fresh();
}
async function more() {
  const before = groups.value;
  await model.next();
  if (groups.value > before) {
    model.choose(groups.value - 1);
    await nextTick();
    resetView();
  }
}
async function commentPage(value: number, prior = false) {
  lastCommentAttempt.value = value;
  await model.readComments(value);
  if (commentsError.value) return;
  if (prior) offsetHistory.value.pop();
  else if (value !== offset()) offsetHistory.value.push(value);
}
</script>

<template>
  <section
    ref="surface"
    class="community-page"
    aria-labelledby="community-title"
    @scroll.passive="rememberScroll"
  >
    <header class="page-heading community-heading">
      <h1 id="community-title" ref="heading" tabindex="-1">
        {{ detailKey ? '动态详情' : '关注' }}
      </h1>
      <div v-if="!detailKey" class="community-heading-actions">
        <CommunityComposer :epoch="accountEpoch" @published-detail="openPublished" />
        <button
          class="icon-button"
          type="button"
          aria-label="刷新关注动态"
          :disabled="pending"
          @click="refresh"
        >
          <Icon name="refresh" />
        </button>
      </div>
    </header>
    <template v-if="!detailKey">
      <CommunityNewSongNotice
        v-if="notice.card.value"
        :card="notice.card.value"
        @open="openNewSongs"
      />
      <p v-if="notice.error.value" class="muted community-notice-error" role="status">
        新歌提示读取失败。
        <button
          class="text-button"
          type="button"
          aria-label="重试读取新歌提示"
          :disabled="notice.pending.value"
          @click="notice.visit()"
        >
          重试
        </button>
      </p>
    </template>

    <template v-if="detailKey">
      <p v-if="detailPending" class="muted" role="status">正在读取动态…</p>
      <div v-else-if="detailError" class="catalog-error" role="alert">
        {{ detailError }}
        <button class="text-button" type="button" @click="model.openDetail(detailKey)">重试</button>
      </div>
      <div v-else-if="detail === null" class="empty-state">
        <p>这条笔记已不存在或无法查看。</p>
        <button class="text-button" type="button" @click="emit('back')">返回关注</button>
      </div>
      <template v-else-if="detail">
        <CommunityEventCard
          :card="detail.card"
          @topic="openTopic"
          :playable="playable"
          @resource="(item) => emit('resource', item)"
        />
        <section v-if="detail.commentsAvailable" class="community-comments" aria-label="动态评论">
          <h2>
            评论 <span v-if="total !== null" class="community-comment-count">{{ total }}</span>
          </h2>
          <p v-if="commentsPending" class="muted" role="status">正在读取评论…</p>
          <div v-if="commentsError" class="catalog-error" role="alert">
            {{ commentsError }}
            <button
              class="text-button"
              type="button"
              @click="commentPage(lastCommentAttempt, lastCommentAttempt < offset())"
            >
              重试评论
            </button>
          </div>
          <p v-if="!commentsPending && !commentsError && total === 0" class="muted">暂无评论</p>
          <h3 v-if="hot.length && offset() === 0">精彩评论</h3>
          <ul v-if="hot.length && offset() === 0">
            <li v-for="comment in hot" :key="comment.id">
              <strong>{{ comment.authorName ?? '网易云音乐用户' }}</strong>
              <p>{{ comment.text }}</p>
            </li>
          </ul>
          <h3 v-if="latest.length">最新评论</h3>
          <ul v-if="latest.length">
            <li v-for="comment in latest" :key="comment.id">
              <strong>{{ comment.authorName ?? '网易云音乐用户' }}</strong>
              <time v-if="comment.time > 0 && comment.time <= 8640000000000000" class="muted">
                {{ date.format(comment.time) }}
              </time>
              <p>{{ comment.text }}</p>
            </li>
          </ul>
          <nav
            v-if="nextOffset !== null || offsetHistory.length > 1"
            class="community-navigation"
            aria-label="评论分页"
          >
            <button
              v-if="offsetHistory.length > 1"
              type="button"
              class="text-button"
              :disabled="commentsPending"
              @click="commentPage(offsetHistory.at(-2) ?? 0, true)"
            >
              上一页评论
            </button>
            <button
              v-if="nextOffset !== null"
              type="button"
              class="text-button"
              :disabled="commentsPending"
              @click="commentPage(nextOffset)"
            >
              下一页评论
            </button>
          </nav>
        </section>
        <p v-else class="muted">暂无法读取这条动态的评论。</p>
      </template>
    </template>
    <template v-else>
      <p v-if="!loaded && pending" class="muted" role="status">正在读取关注动态…</p>
      <div v-if="!loaded && feedError" class="catalog-error" role="alert">
        {{ feedError }}
        <button class="text-button" type="button" @click="model.retry">重试</button>
      </div>
      <p
        v-if="loaded && !pending && !feedError && !visible.length && !continuation"
        class="empty-state"
      >
        暂无关注动态
      </p>
      <div class="community-events">
        <CommunityEventCard
          v-for="card in visible"
          :key="card.key"
          :card="card"
          :playable="playable"
          @open="open"
          @topic="openTopic"
          @resource="(item) => emit('resource', item)"
        />
      </div>
      <p v-if="loaded && pending" class="muted" role="status">正在读取动态…</p>
      <div v-if="loaded && feedError" class="catalog-error" role="alert">
        {{ feedError }} · 已保留当前动态
        <button class="text-button" type="button" @click="model.retry">重试</button>
      </div>
      <nav v-if="groups > 1 || continuation" class="community-navigation" aria-label="动态分页">
        <button
          v-if="group > 0"
          class="text-button"
          type="button"
          :disabled="pending"
          @click="
            model.choose(group - 1);
            resetView();
          "
        >
          较新的动态
        </button>
        <button
          v-if="group + 1 < groups"
          class="text-button"
          type="button"
          :disabled="pending"
          @click="
            model.choose(group + 1);
            resetView();
          "
        >
          较早的动态
        </button>
        <button
          v-else-if="continuation"
          class="text-button"
          type="button"
          :disabled="pending || !!feedError"
          @click="more"
        >
          加载更多动态
        </button>
      </nav>
      <div v-if="autoMore && !pending && !feedError" ref="sentinel" aria-hidden="true"></div>
    </template>
  </section>
</template>

<style scoped>
.community-heading-actions {
  display: flex;
  align-items: center;
  gap: 8px;
}

.community-page {
  max-width: 920px;
  flex: 1;
  min-height: 0;
  min-width: 0;
  overflow: auto;
  overscroll-behavior: contain;
  padding-inline-end: 6px;
}
.community-events {
  display: grid;
  gap: 14px;
}
.community-comments {
  margin-block: 24px;
}
.community-comment-count {
  margin-inline-start: 5px;
  color: var(--text-secondary);
  font-size: 0.8em;
}
.community-comments ul {
  list-style: none;
  padding: 0;
}
.community-comments li {
  border-block-end: 1px solid var(--line);
  padding-block: 10px;
  overflow-wrap: anywhere;
}
.community-comments time {
  margin-inline-start: 10px;
  font-size: 12px;
}
.community-comments li p {
  white-space: pre-wrap;
  margin-block: 6px 0;
}
.community-navigation {
  display: flex;
  justify-content: center;
  gap: 12px;
  flex-wrap: wrap;
  padding-block: 12px;
}
</style>
