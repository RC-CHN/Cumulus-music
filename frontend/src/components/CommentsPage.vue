<script setup lang="ts">
import { usePageEntryFocus } from '../composables/usePageEntryFocus';
import { useCommentReadFocus } from '../composables/useCommentReadFocus';
import { useCommentFloorView } from '../composables/useCommentFloorView';
import { computed, ref, watch, nextTick, onUnmounted, useId } from 'vue';
import type { CommentsModel } from '../composables/useComments';
import CommentCard from './CommentCard.vue';
import CommentWritePanel from './CommentWritePanel.vue';
import CommentFloorPanel from './CommentFloorPanel.vue';
import { useCommentContext, type CommentContext } from '../composables/useCommentContext';
import { resourceKey, type CommentItem } from '../services/comments';
const props = defineProps<{
  model: CommentsModel;
  title: string;
  embedded?: boolean;
  context?: CommentContext;
}>();
const { page, query, loading, error, actionError, busy, history } = props.model;
const context = props.context ?? useCommentContext(props.model);
const { target, floor, writer } = context;
const scroller = ref<HTMLElement>();
const owner = () =>
  `${props.model.epoch.value}:${props.model.resource.value ? resourceKey(props.model.resource.value) : ''}`;
const read = useCommentReadFocus(scroller, owner, () =>
  JSON.stringify([query.value.sort, query.value.offset, query.value.commentId]),
);
const panelId = useId();
function sortKey(event: KeyboardEvent, value: 'latest' | 'hot') {
  const next =
    event.key === 'Home'
      ? 'latest'
      : event.key === 'End'
        ? 'hot'
        : ['ArrowLeft', 'ArrowRight'].includes(event.key)
          ? value === 'latest'
            ? 'hot'
            : 'latest'
          : undefined;
  if (!next || loading.value) return;
  event.preventDefault();
  const target = scroller.value?.querySelector<HTMLButtonElement>(`[data-comment-sort="${next}"]`);
  target?.focus({ preventScroll: true });
  void read(event, () => props.model.sort(next), target ?? undefined);
}
usePageEntryFocus(
  scroller,
  () => !!page.value || !!error.value,
  () => !props.embedded,
);
watch(
  query,
  (value, previous) => {
    // Only a confirmed page/sort/location change resets the reading position.
    // Refreshes and likes retain it; failed requests retain the previous query.
    if (
      value.offset !== previous.offset ||
      value.sort !== previous.sort ||
      value.commentId !== previous.commentId
    ) {
      if (props.embedded) scroller.value?.scrollIntoView({ block: 'start', behavior: 'instant' });
      else scroller.value?.scrollTo({ top: 0, behavior: 'instant' });
    }
  },
  { flush: 'post' },
);
const composer = ref<InstanceType<typeof CommentWritePanel>>(),
  floorContainer = ref<HTMLDivElement>();
const floorView = useCommentFloorView(floor, scroller, floorContainer, owner);
const writing = computed(() => writer.busy.value || writer.pending.value.length > 0);
onUnmounted(() => {
  if (!props.context) context.dispose();
});
async function replyTo(item: CommentItem) {
  writer.setReply(item);
  await nextTick();
  composer.value?.focus();
}
async function floorLike(item: CommentItem) {
  const e = props.model.epoch.value,
    parent = floor.query.value?.parentId;
  await props.model.like(item);
  if (e === props.model.epoch.value && floor.query.value?.parentId === parent) await floor.load();
}
async function floorReconcile(id: string) {
  const e = props.model.epoch.value,
    parent = floor.query.value?.parentId;
  await props.model.reconcile(id);
  if (e === props.model.epoch.value && floor.query.value?.parentId === parent) await floor.load();
}
const groups = computed(() =>
  page.value
    ? [
        {
          key: 'current',
          title: '定位的评论',
          items: page.value.currentComment ? [page.value.currentComment] : [],
        },
        { key: 'top', title: '置顶评论', items: page.value.topComments },
        { key: 'hot', title: '精彩评论', items: page.value.hotComments },
        {
          key: 'page',
          title: query.value.sort === 'hot' ? '精彩评论' : '最新评论',
          items: page.value.comments,
        },
      ]
    : [],
);
const missingTarget = computed(
  () =>
    !!query.value.commentId &&
    !!page.value &&
    !groups.value.some((group) => group.items.some((item) => item.id === query.value.commentId)),
);
</script>
<template>
  <section
    ref="scroller"
    class="comments-page"
    :class="{ 'comments-embedded': embedded }"
    aria-label="评论页面"
    :aria-busy="loading"
  >
    <header class="page-heading">
      <div>
        <p v-if="!embedded" class="muted">{{ title }}</p>
        <component :is="embedded ? 'h2' : 'h1'" data-comment-read-heading tabindex="-1">
          评论 <small v-if="page">{{ page.total }}</small>
        </component>
      </div>
      <button class="text-button" :disabled="loading" @click="read($event, () => model.load())">
        刷新评论
      </button>
    </header>
    <CommentWritePanel
      v-if="model.epoch.value && model.resource.value"
      ref="composer"
      :key="`${model.epoch.value}:${resourceKey(model.resource.value)}`"
      :model="writer"
    />
    <div class="comments-tools">
      <div class="comments-tabs" role="tablist" aria-label="评论排序">
        <button
          role="tab"
          :aria-selected="query.sort === 'latest'"
          :id="`${panelId}-latest`"
          :aria-controls="panelId"
          :tabindex="query.sort === 'latest' ? 0 : -1"
          data-comment-sort="latest"
          :disabled="loading"
          @click="read($event, () => model.sort('latest'))"
          @keydown="sortKey($event, 'latest')"
        >
          最新评论</button
        ><button
          role="tab"
          :aria-selected="query.sort === 'hot'"
          :id="`${panelId}-hot`"
          :aria-controls="panelId"
          :tabindex="query.sort === 'hot' ? 0 : -1"
          data-comment-sort="hot"
          :disabled="loading"
          @click="read($event, () => model.sort('hot'))"
          @keydown="sortKey($event, 'hot')"
        >
          精彩评论
        </button>
      </div>
      <form @submit.prevent="read($event, () => model.locate(target.trim()))">
        <input
          v-model="target"
          aria-label="评论编号"
          placeholder="输入评论编号"
          maxlength="128"
        /><button class="text-button" :disabled="loading || !target.trim()">定位评论</button>
      </form>
    </div>
    <div v-if="actionError" class="error-state" role="alert">
      {{ actionError
      }}<button class="text-button" :disabled="loading" @click="read($event, () => model.load())">
        刷新
      </button>
    </div>
    <div v-if="error" class="error-state comment-read-error" role="alert">
      {{ error
      }}<button
        v-if="model.retryTarget.value"
        class="text-button"
        :disabled="loading"
        @click="read($event, model.retry)"
      >
        重试
      </button>
    </div>
    <p v-if="page?.cached" class="form-warning">当前显示缓存评论，联网刷新后可获取最新内容。</p>
    <p
      v-if="!loading && missingTarget && groups.some((group) => group.items.length)"
      class="form-warning comment-target-missing"
      role="status"
    >
      这条评论暂时无法读取
    </p>
    <div v-if="page?.operations.length" class="comment-pending" role="status">
      <p>部分点赞结果尚未确认，请先核对。</p>
      <button
        v-for="op in page.operations"
        :key="op.id"
        class="text-button"
        :disabled="busy.has(op.id)"
        @click="model.reconcile(op.id)"
      >
        核对评论 {{ op.id }}
      </button>
    </div>
    <div v-if="loading && !page" class="empty-state" role="status">
      <span class="spinner" />正在加载评论…
    </div>
    <div ref="floorContainer">
      <CommentFloorPanel
        :model="floor"
        :owner="owner()"
        :busy="busy"
        :operations="page?.operations ?? []"
        :writing="writing"
        @like="floorLike"
        @reply="replyTo"
        @remove="composer?.requestDelete($event)"
        @locate="model.locate"
        @reconcile="floorReconcile"
        @close="floorView.close"
      />
    </div>
    <div :id="panelId" role="tabpanel" :aria-labelledby="`${panelId}-${query.sort}`">
      <template v-for="group in groups" :key="group.key"
        ><section v-if="group.items.length" class="comment-group" :data-group="group.key">
          <h2>{{ group.title }}</h2>
          <CommentCard
            v-for="item in group.items"
            :key="item.id"
            :item="item"
            :busy="busy.has(item.id)"
            :pending="!!page?.operations.some((op) => op.id === item.id)"
            :writing="writing"
            @like="model.like"
            @locate="model.locate"
            @reconcile="model.reconcile"
            @reply="replyTo"
            @remove="composer?.requestDelete($event)"
            @floor="floorView.open"
          /></section
      ></template>
      <div v-if="page && groups.every((group) => !group.items.length)" class="empty-state">
        {{
          query.commentId
            ? '这条评论暂时无法读取'
            : query.sort === 'hot'
              ? '暂无精彩评论'
              : '还没有评论'
        }}
      </div>
      <footer v-if="page" class="comment-pagination">
        <p v-if="error" class="comment-navigation-error" role="status">
          {{ error
          }}<button
            v-if="model.retryTarget.value"
            class="text-button"
            :disabled="loading"
            @click="read($event, model.retry)"
          >
            重试
          </button>
        </p>
        <button
          class="download-button"
          :disabled="loading || !history.length"
          @click="read($event, model.back)"
        >
          上一页</button
        ><span
          >{{ page.comments.length ? query.offset + 1 : 0
          }}{{ page.comments.length ? `–${query.offset + page.comments.length}` : '' }} /
          {{ page.total }}</span
        ><button
          class="download-button"
          :disabled="loading || !page.next"
          @click="read($event, model.next)"
        >
          下一页
        </button>
      </footer>
    </div>
  </section>
</template>
