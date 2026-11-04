<script setup lang="ts">
import { computed, ref } from 'vue';
import { useCommentReadFocus } from '../composables/useCommentReadFocus';
import type { CommentFloorModel } from '../composables/useCommentFloor';
import type { CommentItem, CommentOperation } from '../services/comments';
import CommentCard from './CommentCard.vue';
const props = defineProps<{
  model: CommentFloorModel;
  owner: string;
  busy: Set<string>;
  operations: CommentOperation[];
  writing: boolean;
}>();
const emit = defineEmits<{
  like: [item: CommentItem];
  reply: [item: CommentItem];
  remove: [item: CommentItem];
  locate: [id: string];
  reconcile: [id: string];
  author: [id: string, event: MouseEvent];
  close: [];
}>();
const { page, query, history, loading, error } = props.model;
const root = ref<HTMLElement>();
const read = useCommentReadFocus(
  root,
  () => `${props.owner}:${query.value?.parentId ?? ''}`,
  () => JSON.stringify(query.value),
);
const groups = computed(() => [
  { key: 'root', title: '原评论', items: page.value?.owner ? [page.value.owner] : [] },
  {
    key: 'current',
    title: '定位的回复',
    items:
      page.value?.currentComment && page.value.currentComment.id !== page.value.owner?.id
        ? [page.value.currentComment]
        : [],
  },
  { key: 'best', title: '精彩回复', items: page.value?.bestComments ?? [] },
  { key: 'replies', title: '全部回复', items: page.value?.comments ?? [] },
]);
</script>
<template>
  <section
    v-if="query"
    ref="root"
    class="comment-floor-panel"
    aria-label="楼层回复"
    :aria-busy="loading"
    :data-parent-id="query.parentId"
  >
    <header>
      <h2 tabindex="-1" data-comment-read-heading>
        评论回复 <small v-if="page">{{ page.total }}</small>
      </h2>
      <button class="text-button" @click="emit('close')">收起回复</button>
    </header>
    <div v-if="error" class="error-state" role="alert">
      {{ error
      }}<button
        class="text-button"
        :disabled="loading || !model.retryTarget.value"
        @click="read($event, model.retry)"
      >
        重试
      </button>
    </div>
    <p v-if="page?.cached" class="form-warning">当前显示缓存回复。</p>
    <p v-if="loading" role="status">正在读取回复…</p>
    <section v-for="group in groups" :key="group.key" :data-floor-group="group.key">
      <h3 v-if="group.items.length">{{ group.title }}</h3>
      <CommentCard
        v-for="item in group.items"
        :key="item.id"
        :item="item"
        :busy="busy.has(item.id)"
        :pending="operations.some((v) => v.id === item.id)"
        :writing="writing"
        :show-floor="false"
        @like="emit('like', $event)"
        @reply="emit('reply', $event)"
        @remove="emit('remove', $event)"
        @locate="emit('locate', $event)"
        @reconcile="emit('reconcile', $event)"
        @author="(id, event) => emit('author', id, event)"
      />
    </section>
    <p v-if="page && !page.comments.length" class="muted">暂无回复。</p>
    <footer v-if="page">
      <p v-if="error" class="comment-navigation-error" role="status">
        {{ error
        }}<button
          class="text-button"
          :disabled="loading || !model.retryTarget.value"
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
        上一页回复</button
      ><button
        class="download-button"
        :disabled="loading || !page.next"
        @click="read($event, model.next)"
      >
        下一页回复
      </button>
    </footer>
  </section>
</template>
<style scoped>
.comment-floor-panel {
  margin: 24px 0;
  border: 1px solid var(--line);
  border-radius: 12px;
  padding: 18px;
  background: var(--surface);
}
.comment-floor-panel > header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}
.comment-floor-panel h2 {
  font-size: 17px;
  margin: 0;
}
.comment-floor-panel h3 {
  font-size: 14px;
  color: var(--text-secondary);
  margin: 18px 0 0;
}
.comment-floor-panel small {
  font-weight: 400;
  color: var(--text-secondary);
}
.comment-floor-panel footer {
  display: flex;
  justify-content: center;
  flex-wrap: wrap;
  gap: 16px;
  margin-top: 18px;
}
</style>
