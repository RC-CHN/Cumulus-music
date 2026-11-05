<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import type { TrackView } from '../services/contracts';
import type { VinylCommentsModel } from '../composables/useVinylComments';
import CommentText from './CommentText.vue';
import Icon from './Icon.vue';

const props = defineProps<{
  track: TrackView;
  active: boolean;
  count?: number;
  model: VinylCommentsModel;
}>();
const emit = defineEmits<{ back: []; full: []; hot: [] }>();
const scroller = ref<HTMLElement>();
const next = ref<HTMLElement>();
const back = ref<HTMLButtonElement>();
const artists = computed(() => props.track.artists.join(' / '));
const groups = computed(() => [
  { key: 'hot', comments: props.model.hot.value },
  { key: 'latest', comments: props.model.latest.value },
]);
watch(
  [next, scroller, () => props.model.canAppend.value],
  ([target, root, ready], _, cleanup) => {
    if (!target || !root || !ready) return;
    const observer = new IntersectionObserver(
      (entries) => {
        if (entries.some((entry) => entry.isIntersecting) && props.model.canAppend.value)
          void props.model.load();
      },
      { root, rootMargin: '50px', threshold: 0 },
    );
    observer.observe(target);
    cleanup(() => observer.disconnect());
  },
  { flush: 'post' },
);
watch(
  () => props.track.id,
  () => scroller.value?.scrollTo({ top: 0, behavior: 'instant' }),
);
function wheel(event: WheelEvent) {
  if ((scroller.value?.scrollTop ?? 0) > 0) event.stopPropagation();
}
function focusBack() {
  back.value?.focus({ preventScroll: true });
}
defineExpose({ focusBack });
</script>

<template>
  <section class="vinyl-comments-pane" aria-label="播放详情评论" :inert="!active">
    <header class="vinyl-comments-track">
      <button
        ref="back"
        type="button"
        class="text-button vinyl-comments-back"
        aria-label="返回歌词"
        @click="emit('back')"
      >
        <img v-if="track.coverUrl" :src="track.coverUrl" alt="" referrerpolicy="no-referrer" />
        <Icon v-else name="music" />
        <span :title="track.name"
          >{{ track.name }}<small v-if="artists"> · {{ artists }}</small></span
        >
        <Icon name="up" />
      </button>
    </header>
    <div ref="scroller" class="vinyl-comments-scroll" @wheel.passive="wheel">
      <header class="vinyl-comments-heading">
        <h2>
          全部评论 <small v-if="count !== undefined">{{ count }}</small>
        </h2>
        <button type="button" class="text-button" @click="emit('full')">打开独立评论页</button>
      </header>
      <p v-if="model.failed.value && !model.loaded.value" class="empty-state">
        获取评论数据失败。<button class="text-button" type="button" @click="model.load">
          重试
        </button>
      </p>
      <p v-else-if="count === 0" class="empty-state">暂无评论</p>
      <p v-else-if="count === undefined" class="empty-state">暂无评论信息</p>
      <p v-else-if="!model.loaded.value" class="empty-state" role="status">正在加载评论…</p>
      <template v-else>
        <section v-for="group in groups" :key="group.key" :data-vinyl-comment-group="group.key">
          <h3 v-if="group.key === 'latest' && model.hot.value.length">最新评论</h3>
          <article
            v-for="(item, index) in group.comments"
            :key="`${item.id}:${index}`"
            class="vinyl-comment-row"
          >
            <img
              v-if="item.author?.avatarUrl"
              :src="item.author.avatarUrl"
              alt=""
              referrerpolicy="no-referrer"
              loading="lazy"
              decoding="async"
            />
            <Icon v-else name="user" />
            <div>
              <strong>{{ item.author?.name || '云音乐用户' }}</strong>
              <p><CommentText :text="item.content" /></p>
              <small>{{ item.timeText }}</small>
            </div>
          </article>
          <button
            v-if="group.key === 'hot' && model.moreHot.value"
            type="button"
            class="text-button vinyl-more-hot"
            @click="emit('hot')"
          >
            查看更多精彩评论
          </button>
        </section>
        <p v-if="model.failed.value" class="empty-state">
          加载更多评论失败。<button class="text-button" type="button" @click="model.load">
            重试
          </button>
        </p>
        <p v-else-if="model.working.value" role="status">正在加载更多评论…</p>
        <div ref="next" aria-hidden="true" class="vinyl-comments-sentinel" />
      </template>
    </div>
  </section>
</template>
