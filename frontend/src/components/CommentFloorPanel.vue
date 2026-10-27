<script setup lang="ts">
import { computed } from 'vue';
import type { CommentFloorModel } from '../composables/useCommentFloor';
import type { CommentItem, CommentOperation } from '../services/comments';
import CommentCard from './CommentCard.vue';
const props = defineProps<{
  model: CommentFloorModel;
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
}>();
const { page, query, history, loading, error } = props.model;
const groups = computed(() => [
  { key: 'root', title: '原评论', items: page.value?.owner ? [page.value.owner] : [] },
  { key: 'best', title: '精彩回复', items: page.value?.bestComments ?? [] },
  { key: 'replies', title: '全部回复', items: page.value?.comments ?? [] },
]);
</script>
<template>
  <section
    v-if="query"
    class="comment-floor-panel"
    aria-label="楼层回复"
    :data-parent-id="query.parentId"
  >
    <header>
      <h2>
        评论回复 <small v-if="page">{{ page.total }}</small>
      </h2>
      <button class="text-button" @click="model.close">收起回复</button>
    </header>
    <div v-if="error" class="error-state" role="alert">
      {{ error }}<button class="text-button" :disabled="loading" @click="model.load()">重试</button>
    </div>
    <p v-if="page?.cached" class="form-warning">当前显示缓存回复。</p>
    <p v-if="loading" role="status">正在读取回复…</p>
    <section v-for="group in groups" :key="group.key">
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
      />
    </section>
    <p v-if="page && !page.comments.length" class="muted">暂无回复。</p>
    <footer v-if="page">
      <button class="download-button" :disabled="loading || !history.length" @click="model.back">
        上一页回复</button
      ><button class="download-button" :disabled="loading || !page.next" @click="model.next">
        下一页回复
      </button>
    </footer>
  </section>
</template>
<style scoped>
.comment-floor-panel {
  margin: 24px 0;
  border: 1px solid #eee;
  border-radius: 12px;
  padding: 18px;
  background: #fcfbfd;
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
  color: #777;
  margin: 18px 0 0;
}
.comment-floor-panel small {
  font-weight: 400;
  color: #888;
}
.comment-floor-panel footer {
  display: flex;
  justify-content: center;
  flex-wrap: wrap;
  gap: 16px;
  margin-top: 18px;
}
</style>
