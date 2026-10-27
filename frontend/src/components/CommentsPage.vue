<script setup lang="ts">
import { computed, ref, watch, nextTick, onUnmounted } from 'vue';
import type { CommentsModel } from '../composables/useComments';
import CommentCard from './CommentCard.vue';
import CommentWritePanel from './CommentWritePanel.vue';
import CommentFloorPanel from './CommentFloorPanel.vue';
import { useCommentWrites } from '../composables/useCommentWrites';
import { useCommentFloor } from '../composables/useCommentFloor';
import { resourceKey, type CommentItem } from '../services/comments';
const props = defineProps<{ model: CommentsModel; title: string }>();
const { page, query, loading, error, actionError, busy, history } = props.model;
const target = ref('');
const composer = ref<InstanceType<typeof CommentWritePanel>>(),
  floorContainer = ref<HTMLDivElement>();
const floor = useCommentFloor(undefined, () => props.model.sessionExpired());
const writer = useCommentWrites(
  undefined,
  async (row) => {
    const e = props.model.epoch.value,
      k = resourceKey(row.resource);
    if (row.command.kind === 'publish' && row.receiptId) await props.model.locate(row.receiptId);
    else await props.model.load();
    if (
      e === props.model.epoch.value &&
      props.model.resource.value &&
      resourceKey(props.model.resource.value) === k &&
      floor.query.value
    )
      await floor.load();
  },
  () => props.model.invalidate(),
  () => props.model.sessionExpired(),
);
const writing = computed(() => writer.busy.value || writer.pending.value.length > 0);
watch(
  () => [
    props.model.epoch.value,
    props.model.resource.value ? resourceKey(props.model.resource.value) : '',
  ],
  () => {
    const e = props.model.epoch.value,
      r = props.model.resource.value;
    floor.connect(e, r);
    void writer.connect(e, r);
  },
  { immediate: true },
);
onUnmounted(() => {
  writer.disconnect();
  floor.connect('');
});
async function replyTo(item: CommentItem) {
  writer.setReply(item);
  await nextTick();
  composer.value?.focus();
}
async function openFloor(item: CommentItem) {
  await floor.open(item);
  await nextTick();
  if (floor.query.value?.parentId === (item.parentId ?? item.id))
    floorContainer.value?.scrollIntoView({ block: 'start', behavior: 'smooth' });
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
</script>
<template>
  <section class="comments-page" aria-label="评论页面">
    <header class="page-heading">
      <div>
        <p class="muted">{{ title }}</p>
        <h1>
          评论 <small v-if="page">{{ page.total }}</small>
        </h1>
      </div>
      <button class="text-button" :disabled="loading" @click="model.load()">刷新评论</button>
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
          :disabled="loading"
          @click="model.sort('latest')"
        >
          最新评论</button
        ><button
          role="tab"
          :aria-selected="query.sort === 'hot'"
          :disabled="loading"
          @click="model.sort('hot')"
        >
          精彩评论
        </button>
      </div>
      <form @submit.prevent="model.locate(target.trim())">
        <input
          v-model="target"
          aria-label="评论编号"
          placeholder="输入评论编号"
          maxlength="128"
        /><button class="text-button" :disabled="loading || !target.trim()">定位评论</button>
      </form>
    </div>
    <div v-if="error || actionError" class="error-state" role="alert">
      {{ actionError || error
      }}<button class="text-button" :disabled="loading" @click="model.load()">刷新</button>
    </div>
    <p v-if="page?.cached" class="form-warning">当前显示缓存评论，联网刷新后可获取最新内容。</p>
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
        :busy="busy"
        :operations="page?.operations ?? []"
        :writing="writing"
        @like="floorLike"
        @reply="replyTo"
        @remove="composer?.requestDelete($event)"
        @locate="model.locate"
        @reconcile="floorReconcile"
      />
    </div>
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
          @floor="openFloor"
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
      <button class="download-button" :disabled="loading || !history.length" @click="model.back">
        上一页</button
      ><span
        >{{ page.comments.length ? query.offset + 1 : 0
        }}{{ page.comments.length ? `–${query.offset + page.comments.length}` : '' }} /
        {{ page.total }}</span
      ><button class="download-button" :disabled="loading || !page.next" @click="model.next">
        下一页
      </button>
    </footer>
  </section>
</template>
