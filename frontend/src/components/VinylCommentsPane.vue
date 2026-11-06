<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import type { TrackView } from '../services/contracts';
import { vinylLikeCount, type CommentItem } from '../services/comments';
import type { VinylComposerTarget } from '../composables/useVinylCommentComposer';
import type { VinylCommentsModel } from '../composables/useVinylComments';
import CommentText from './CommentText.vue';
import CommentReportMenu from './CommentReportMenu.vue';
import Icon from './Icon.vue';

const props = defineProps<{
  track: TrackView;
  active: boolean;
  count?: number;
  model: VinylCommentsModel;
  writeTarget?: VinylComposerTarget;
}>();
const emit = defineEmits<{
  back: [];
  full: [];
  hot: [];
  reportExpired: [];
  compose: [target: VinylComposerTarget, reply: CommentItem | undefined, opener: HTMLElement];
}>();
const scroller = ref<HTMLElement>();
const next = ref<HTMLElement>();
const back = ref<HTMLButtonElement>();
const reportMenus = new Map<string, InstanceType<typeof CommentReportMenu>>();
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
function rowKey(group: string, id: string, index: number) {
  return `${group}:${id}:${index}`;
}
function bindReportMenu(key: string, menu: unknown) {
  if (menu) reportMenus.set(key, menu as InstanceType<typeof CommentReportMenu>);
  else reportMenus.delete(key);
}
function compose(event: MouseEvent, item?: CommentItem) {
  const target = props.writeTarget;
  const opener = event.currentTarget;
  if (
    !props.active ||
    !target ||
    target.resource.id !== props.track.id ||
    !(opener instanceof HTMLElement) ||
    !opener.isConnected ||
    (item && !props.model.hot.value.includes(item) && !props.model.latest.value.includes(item))
  )
    return;
  emit('compose', target, item, opener);
}
defineExpose({ focusBack });
</script>

<template>
  <section
    class="vinyl-comments-pane"
    :class="{ 'vinyl-comments-writable': writeTarget }"
    aria-label="播放详情评论"
    :inert="!active"
  >
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
      <p
        v-if="model.failed.value && !model.loaded.value && !model.latest.value.length"
        class="empty-state"
      >
        获取评论数据失败。<button class="text-button" type="button" @click="model.load">
          重试
        </button>
      </p>
      <p v-else-if="count === 0 && !model.latest.value.length" class="empty-state">暂无评论</p>
      <p v-else-if="count === undefined" class="empty-state">暂无评论信息</p>
      <p
        v-else-if="!model.loaded.value && !model.latest.value.length"
        class="empty-state"
        role="status"
      >
        正在加载评论…
      </p>
      <template v-else>
        <p v-if="writeTarget && model.likes.readFailed.value" class="empty-state" role="status">
          无法读取点赞状态。
          <button type="button" class="text-button" @click="model.likes.refresh">重新读取</button>
        </p>
        <section v-for="group in groups" :key="group.key" :data-vinyl-comment-group="group.key">
          <h3 v-if="group.key === 'latest' && model.hot.value.length">最新评论</h3>
          <article
            v-for="(item, index) in group.comments"
            :key="`${item.id}:${index}`"
            class="vinyl-comment-row"
            :data-vinyl-comment-id="item.id"
            @contextmenu="reportMenus.get(rowKey(group.key, item.id, index))?.contextMenu($event)"
            @keydown.shift.f10="
              reportMenus.get(rowKey(group.key, item.id, index))?.contextMenu($event)
            "
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
              <p data-vinyl-copy-text><CommentText :text="item.content" /></p>
              <blockquote
                v-if="item.replies[0]?.content"
                class="vinyl-comment-quote"
                data-vinyl-copy-text
              >
                {{ `@${item.replies[0].author?.name || '云音乐用户'}: `
                }}<CommentText :text="item.replies[0].content" />
              </blockquote>
              <div class="vinyl-comment-actions">
                <small>{{ item.timeText }}</small>
                <div v-if="writeTarget" class="vinyl-comment-controls">
                  <CommentReportMenu
                    v-if="item.reportable && !item.owned"
                    :ref="(menu) => bindReportMenu(rowKey(group.key, item.id, index), menu)"
                    :epoch="writeTarget.epoch"
                    :resource="writeTarget.resource"
                    :vinyl-play-id="writeTarget.playId"
                    :item="item"
                    :can-delete="false"
                    @expired="emit('reportExpired')"
                  />
                  <button
                    v-if="model.likes.operations.value.has(item.id)"
                    type="button"
                    class="text-button vinyl-comment-like-reconcile"
                    :disabled="!!model.likes.busy.value || !model.likes.ready.value"
                    @click="model.likes.reconcile(item)"
                  >
                    核对点赞状态
                  </button>
                  <button
                    type="button"
                    class="icon-button vinyl-comment-like"
                    :class="{ liked: item.liked === true }"
                    :aria-label="`${item.liked ? '取消评论点赞' : '点赞评论'} ${item.id}`"
                    :aria-pressed="item.liked ?? undefined"
                    :disabled="
                      item.liked === null ||
                      model.likes.operations.value.has(item.id) ||
                      !!model.likes.busy.value ||
                      model.likes.reading.value ||
                      !model.likes.ready.value
                    "
                    @click="model.likes.toggle(item)"
                  >
                    <span v-if="item.likedCount">{{ vinylLikeCount(item.likedCount) }}</span>
                    <Icon name="thumb" />
                  </button>
                  <button
                    type="button"
                    class="text-button vinyl-comment-reply"
                    :aria-label="`回复 ${item.author?.name || '云音乐用户'}`"
                    @click="compose($event, item)"
                  >
                    回复
                  </button>
                </div>
              </div>
              <p v-if="writeTarget && model.likes.issue.value?.id === item.id" role="status">
                {{ model.likes.issue.value.message }}
              </p>
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
    <div v-if="writeTarget" class="vinyl-comment-publish">
      <button type="button" class="secondary" @click="compose"><Icon name="edit" />发布评论</button>
    </div>
  </section>
</template>
