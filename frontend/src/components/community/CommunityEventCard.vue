<script setup lang="ts">
import type { CommunityCard, CommunityResource } from '../../services/community';
import Icon from '../Icon.vue';

const props = defineProps<{ card: CommunityCard; nested?: boolean; playable: boolean }>();
const emit = defineEmits<{
  open: [id: string];
  resource: [value: CommunityResource];
}>();
const date = new Intl.DateTimeFormat('zh-CN', { dateStyle: 'medium', timeStyle: 'short' });
const labels = {
  track: '单曲',
  playlist: '歌单',
  album: '专辑',
  artist: '歌手',
  podcast: '播客',
  voice: '声音',
};
</script>

<template>
  <article class="community-event-card" :data-event-key="nested ? undefined : card.key">
    <header class="community-author">
      <img
        v-if="card.avatarUrl"
        :src="card.avatarUrl"
        alt=""
        loading="lazy"
        referrerpolicy="no-referrer"
      />
      <span v-else class="community-avatar-fallback" aria-hidden="true"><Icon name="user" /></span>
      <div>
        <strong>{{ card.authorName }}</strong>
        <p class="muted">
          {{ card.timeText ?? (card.eventTime !== null ? date.format(card.eventTime) : '') }}
        </p>
      </div>
      <button
        v-if="card.detail"
        :data-community-opener="card.detail"
        class="text-button community-open"
        type="button"
        :aria-label="`查看 ${card.authorName} 的动态详情`"
        @click="emit('open', card.detail)"
      >
        查看详情
      </button>
    </header>
    <p v-if="card.status === 'unsupported'" class="muted">
      该笔记类型暂不支持展示，请前往 App 查看。
    </p>
    <p v-else-if="card.status === 'unavailable'" class="muted">暂时无法显示这条动态。</p>
    <template v-else>
      <h2 v-if="card.title" class="community-event-title">{{ card.title }}</h2>
      <p v-if="card.message" class="community-event-text">{{ card.message }}</p>
      <div v-if="card.pictures.length" class="community-pictures">
        <img
          v-for="(url, index) in card.pictures"
          :key="`${index}:${url}`"
          :src="url"
          :alt="`动态配图 ${index + 1}`"
          loading="lazy"
          referrerpolicy="no-referrer"
        />
      </div>
      <p v-if="card.status === 'deleted'" class="muted">该动态已删除</p>
      <template v-else-if="card.attachment">
        <button
          v-if="card.attachment.type === 'resource'"
          type="button"
          class="community-resource"
          :disabled="['track', 'voice'].includes(card.attachment.kind) && !playable"
          :aria-label="`${['track', 'voice'].includes(card.attachment.kind) ? '播放' : '打开'} ${card.attachment.label}`"
          @click="emit('resource', card.attachment)"
        >
          <img
            v-if="card.attachment.coverUrl"
            :src="card.attachment.coverUrl"
            alt=""
            loading="lazy"
          />
          <Icon v-else name="music" />
          <span
            ><small class="muted">{{ labels[card.attachment.kind] }}</small
            >{{ card.attachment.label }}</span
          >
        </button>
        <CommunityEventCard
          v-else-if="card.attachment.type === 'forwarded' && !props.nested"
          :card="card.attachment.card"
          :playable="playable"
          nested
          @resource="(value) => emit('resource', value)"
        />
        <p v-else-if="card.attachment.type === 'unavailable'" class="muted">
          {{ card.attachment.label }}
        </p>
      </template>
    </template>
    <footer v-if="!nested && card.status === 'ready'" class="community-counts muted">
      <span v-if="card.counters.comments !== null">评论 {{ card.counters.comments }}</span>
      <span v-if="card.counters.likes !== null">赞 {{ card.counters.likes }}</span>
      <span v-if="card.counters.shares !== null">分享 {{ card.counters.shares }}</span>
    </footer>
  </article>
</template>

<style scoped>
.community-event-card {
  border: 1px solid var(--line);
  border-radius: 10px;
  padding: 16px;
  display: grid;
  gap: 10px;
  overflow-wrap: anywhere;
}
.community-author {
  display: flex;
  align-items: center;
  gap: 10px;
}
.community-author img,
.community-avatar-fallback {
  width: 42px;
  height: 42px;
  border-radius: 50%;
  flex: none;
  object-fit: cover;
}
.community-avatar-fallback {
  display: grid;
  place-items: center;
  color: var(--text-secondary);
  background: var(--surface-alt);
}
.community-author p,
.community-event-card > p {
  margin: 0;
}
.community-author p {
  font-size: 12px;
  margin-block-start: 3px;
}
.community-open {
  margin-inline-start: auto;
  flex: none;
}
.community-event-title {
  margin: 0;
  font-size: 17px;
}
.community-event-text {
  white-space: pre-wrap;
  line-height: 1.55;
}
.community-pictures {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 108px));
  gap: 6px;
}
.community-pictures img {
  display: block;
  width: 100%;
  aspect-ratio: 1;
  object-fit: cover;
  border-radius: 7px;
}
.community-resource {
  display: inline-flex;
  justify-self: start;
  align-items: center;
  gap: 9px;
  min-width: 0;
  text-align: start;
  color: inherit;
  background: var(--surface-alt);
  border-radius: 8px;
  padding: 8px;
}
.community-resource:hover {
  background: var(--surface-hover);
}
.community-resource span {
  display: grid;
  gap: 3px;
}
.community-resource img {
  width: 36px;
  height: 36px;
  object-fit: cover;
  border-radius: 4px;
}
.community-counts {
  display: flex;
  flex-wrap: wrap;
  gap: 14px;
  font-size: 12px;
}
</style>
