<script setup lang="ts">
import type { CommentItem } from '../services/comments';
import Icon from './Icon.vue';
function timestamp(time: number): string | undefined {
  const date = new Date(time);
  return Number.isFinite(date.getTime()) ? date.toISOString() : undefined;
}
function localTime(time: number): string {
  return timestamp(time) ? new Date(time).toLocaleString() : '时间未知';
}
withDefaults(
  defineProps<{
    item: CommentItem;
    busy: boolean;
    pending: boolean;
    writing?: boolean;
    showFloor?: boolean;
  }>(),
  { writing: false, showFloor: true },
);
const emit = defineEmits<{
  like: [item: CommentItem];
  locate: [id: string];
  reconcile: [id: string];
  reply: [item: CommentItem];
  remove: [item: CommentItem];
  floor: [item: CommentItem];
}>();
</script>
<template>
  <article class="comment-card" :data-comment-id="item.id">
    <img
      v-if="item.author?.avatarUrl"
      class="comment-avatar"
      :src="item.author.avatarUrl"
      alt=""
      loading="lazy"
      referrerpolicy="no-referrer"
    />
    <span v-else class="comment-avatar"><Icon name="user" /></span>
    <div class="comment-copy">
      <div class="comment-author">
        {{ item.author?.name || '云音乐用户' }}<small v-if="item.owned">我</small>
      </div>
      <p class="comment-content">{{ item.content }}</p>
      <div v-for="(reply, index) in item.replies" :key="index" class="comment-quote">
        <span>{{ reply.author?.name || '云音乐用户' }}：</span
        >{{ reply.content ?? '原评论暂不可见' }}
        <button v-if="reply.id" class="text-button" @click="emit('locate', reply.id)">
          查看原评论
        </button>
      </div>
      <div class="comment-footer">
        <time :datetime="timestamp(item.time)">{{ item.timeText || localTime(item.time) }}</time>
        <div class="comment-operations">
          <button v-if="showFloor" class="text-button" @click="emit('floor', item)">
            查看回复
          </button>
          <button class="text-button" :disabled="writing || busy" @click="emit('reply', item)">
            回复
          </button>
          <button
            v-if="item.owned"
            class="text-button comment-delete"
            :disabled="writing || busy || pending"
            @click="emit('remove', item)"
          >
            删除
          </button>
          <button
            class="text-button"
            :aria-label="`定位评论 ${item.id}`"
            @click="emit('locate', item.id)"
          >
            定位
          </button>
          <button
            v-if="pending"
            class="text-button"
            :disabled="busy"
            @click="emit('reconcile', item.id)"
          >
            核对点赞状态
          </button>
          <button
            class="comment-like"
            :class="{ liked: item.liked }"
            :aria-label="`${item.liked ? '取消评论点赞' : '点赞评论'} ${item.id}`"
            :aria-pressed="item.liked ?? false"
            :disabled="writing || busy || pending || item.liked === null"
            @click="emit('like', item)"
          >
            <Icon name="thumb" /><span>{{ item.likedCount || '' }}</span>
          </button>
        </div>
      </div>
    </div>
  </article>
</template>
