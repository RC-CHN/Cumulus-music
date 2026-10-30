<script setup lang="ts">
import type { TrackMenuRequest } from '../services/trackMenu';
import { computed } from 'vue';
import type { useMember } from '../composables/useMember';
import type { MemberTarget, MemberContent } from '../services/member';
import type { TrackView } from '../services/contracts';
import Icon from './Icon.vue';
import MemberCards from './MemberCards.vue';
import TrackTable from './TrackTable.vue';
const props = defineProps<{
  model: ReturnType<typeof useMember>;
  nickname: string | null;
  playable: boolean;
  liked: Set<string>;
  busyLikes: Set<string>;
  currentId?: string;
}>();
const emit = defineEmits<{
  open: [target: MemberTarget];
  play: [section: string | null, index: number];
  playPlaylist: [id: string, startId: string | null];
  like: [track: TrackView];
  more: [request: TrackMenuRequest];
  comments: [track: TrackView];
  download: [tracks: TrackView[]];
}>();
const page = computed(() => props.model.view.value?.page);
const top = computed(() =>
  page.value?.kind === 'home'
    ? page.value.modules.find((m) => m.content.kind === 'banners')
    : undefined,
);
const primary = computed(() => {
  const rights = props.model.rights.value?.entitlements;
  return (
    rights?.find((e) => e.kind === 'plus' && e.active) ??
    rights?.find((e) => e.kind === 'vinyl' && e.active) ??
    rights?.find((e) => e.active)
  );
});
function date(ms: number) {
  return ms > 0 ? new Date(ms).toLocaleDateString('zh-CN') : '';
}
function hasContent(content: MemberContent) {
  return (
    ('tracks' in content ? content.tracks : 'groups' in content ? content.groups : content.cards)
      .length > 0
  );
}
</script>
<template>
  <section class="member-page" :aria-busy="model.loading.value">
    <header class="member-heading">
      <h1>会员中心</h1>
      <button
        class="icon-button"
        aria-label="刷新会员内容"
        :disabled="model.loading.value"
        @click="model.refresh"
      >
        <Icon name="refresh" />
      </button>
    </header>
    <div class="member-tabs" aria-label="会员内容">
      <button
        :aria-pressed="model.query.value.kind === 'home'"
        @click="model.open({ kind: 'home' })"
      >
        VIP精选</button
      ><button
        :aria-pressed="model.query.value.kind === 'history'"
        @click="model.open({ kind: 'history', date: null })"
      >
        历史日推
      </button>
    </div>
    <div v-if="model.query.value.kind === 'home'" class="member-hero">
      <div class="member-rights-card">
        <span class="member-brand">{{
          primary?.name ?? (model.rights.value ? '会员权益' : '正在读取会员权益…')
        }}</span>
        <strong
          >{{ nickname || '我的账号'
          }}<span v-if="primary?.level"> Lv.{{ primary.level }}</span></strong
        >
        <small v-if="primary">有效期至 {{ date(primary.expiresAt) }}</small
        ><small v-else-if="model.rights.value">当前没有生效中的会员权益</small>
        <button
          class="member-rights-refresh"
          :disabled="model.rightsLoading.value"
          @click="model.loadRights(true)"
        >
          {{ model.rightsLoading.value ? '正在更新…' : '刷新权益' }}
        </button>
      </div>
      <MemberCards
        v-if="model.query.value.kind === 'home' && top?.content.kind === 'banners'"
        :cards="top.content.cards"
        banners
        :busy="model.linkBusy.value"
        @open="emit('open', $event)"
      />
      <div v-else class="member-history-intro">
        <strong>发现会员专属好音乐</strong>
        <p>为你精选歌曲、歌单与有声书。</p>
      </div>
    </div>
    <p v-if="model.rightsError.value" class="member-error" role="alert">
      {{ model.rightsError.value
      }}<button class="text-button" @click="model.loadRights(true)">重试权益读取</button>
    </p>
    <div
      v-if="model.query.value.kind === 'home' && model.rights.value"
      class="member-entitlements"
      aria-label="账号会员权益"
    >
      <div
        v-for="right in model.rights.value.entitlements"
        :key="right.kind"
        :data-entitlement="right.kind"
        :data-active="right.active"
      >
        <strong>{{ right.name }}</strong
        ><span>{{ right.active ? '生效中' : right.expiresAt > 0 ? '已到期' : '未开通' }}</span
        ><small v-if="right.expiresAt > 0">{{ date(right.expiresAt) }}</small>
      </div>
    </div>
    <p v-if="model.query.value.kind === 'home'" class="member-quality-note">
      播放与下载按当前选择的音质请求，实际音质和可播放范围以歌曲授权结果为准。
    </p>
    <div v-if="model.error.value" class="member-error" role="alert">
      {{ model.error.value }}<span v-if="page"> · 保留上次加载的内容</span
      ><button class="text-button" :disabled="model.loading.value" @click="model.refresh">
        重试
      </button>
    </div>
    <p v-if="model.view.value?.warning" role="status">{{ model.view.value.warning }}</p>
    <p v-if="model.loading.value" role="status">正在加载会员内容…</p>
    <template v-if="page?.kind === 'home' && model.query.value.kind === 'home'">
      <div v-if="page.messages.length" class="member-messages">
        <span v-for="(message, i) in page.messages" :key="i">{{ message }}</span>
      </div>
      <template v-for="module in page.modules" :key="module.key">
        <section
          v-if="module.key !== top?.key && hasContent(module.content)"
          class="member-module"
          :data-member-module="module.key"
          :data-member-kind="module.content.kind"
        >
          <div v-if="module.content.kind !== 'banners'" class="member-section-title">
            <h2>{{ module.title }}</h2>
            <button
              v-if="module.content.kind === 'tracks'"
              class="icon-button"
              :aria-label="`播放全部 ${module.title}`"
              :disabled="!playable || !module.content.tracks.length"
              @click="emit('play', module.key, 0)"
            >
              <Icon name="play" />
            </button>
          </div>
          <div v-if="module.content.kind === 'tracks'" class="member-tracks">
            <div
              v-for="(track, index) in module.content.tracks"
              :key="`${index}:${track.id}`"
              class="member-track"
              :data-track-id="track.id"
              :class="{ current: track.id === currentId }"
            >
              <button
                class="member-track-play"
                :aria-label="`播放 ${track.name}`"
                :disabled="!playable || track.missing"
                @click="emit('play', module.key, index)"
              >
                <img
                  v-if="track.coverUrl"
                  :src="track.coverUrl"
                  alt=""
                  loading="lazy"
                  referrerpolicy="no-referrer"
                /><Icon v-else name="music" /><span
                  ><strong>{{ track.name }}</strong
                  ><small>{{ track.artists.join(' / ') }}</small></span
                >
              </button>
              <button
                class="icon-button"
                :aria-label="`${liked.has(track.id) ? '取消喜欢' : '喜欢'} ${track.name}`"
                :disabled="busyLikes.has(track.id)"
                @click="emit('like', track)"
              >
                <Icon name="heart" :class="{ liked: liked.has(track.id) }" />
              </button>
              <button
                class="icon-button"
                :aria-label="`评论 ${track.name}`"
                @click="emit('comments', track)"
              >
                <Icon name="comment" />
              </button>
              <button
                class="icon-button"
                :aria-label="`下载 ${track.name}`"
                @click="emit('download', [track])"
              >
                <Icon name="download" />
              </button>
            </div>
          </div>
          <div v-else-if="module.content.kind === 'rankings'" class="member-ranks">
            <article v-for="group in module.content.groups" :key="group.id" class="member-rank">
              <header>
                <button
                  class="text-button"
                  @click="emit('open', { kind: 'playlist', id: group.id })"
                >
                  {{ group.title }}</button
                ><button
                  class="icon-button"
                  :aria-label="`播放榜单 ${group.title}`"
                  :disabled="!playable"
                  @click="emit('playPlaylist', group.id, null)"
                >
                  <Icon name="play" />
                </button>
              </header>
              <button
                v-for="(track, i) in group.tracks"
                :key="`${i}:${track.id}`"
                class="member-rank-track"
                :disabled="!playable || track.missing"
                @click="emit('playPlaylist', group.id, track.id)"
              >
                <span>{{ i + 1 }}</span
                ><img
                  v-if="track.coverUrl"
                  :src="track.coverUrl"
                  alt=""
                  loading="lazy"
                  referrerpolicy="no-referrer"
                /><span
                  ><strong>{{ track.name }}</strong
                  ><small>{{ track.artists.join(' / ') }}</small></span
                >
              </button>
            </article>
          </div>
          <MemberCards
            v-else
            :cards="
              module.content.kind === 'albums'
                ? module.content.cards.slice(0, 6)
                : module.content.cards
            "
            :banners="module.content.kind === 'banners'"
            :busy="model.linkBusy.value"
            @open="emit('open', $event)"
          />
        </section>
      </template>
      <p v-if="!page.modules.length" class="empty-state">暂时没有会员推荐内容</p>
    </template>
    <template v-else-if="page?.kind === 'history'">
      <div class="member-dates" aria-label="历史推荐日期">
        <button
          v-for="item in page.dates"
          :key="item"
          :aria-pressed="page.date === item"
          @click="model.open({ kind: 'history', date: item })"
        >
          {{ item }}
        </button>
      </div>
      <div class="member-history-actions">
        <button
          class="primary"
          :disabled="!playable || !page.tracks.length"
          @click="emit('play', null, 0)"
        >
          <Icon name="play" />播放全部</button
        ><button :disabled="!page.tracks.length" @click="emit('download', page.tracks)">
          下载全部</button
        ><span>{{ page.tracks.length }} 首</span>
      </div>
      <TrackTable
        v-if="page.tracks.length"
        :tracks="page.tracks"
        :liked="liked"
        :busy="busyLikes"
        :playable="playable"
        :current-id="currentId"
        @play="emit('play', null, $event)"
        @like="emit('like', $event)"
        @more="emit('more', $event)"
      />
      <p v-else class="empty-state">
        {{ page.dates.length ? '这一天没有推荐歌曲' : '暂无可查看的历史推荐' }}
      </p>
    </template>
  </section>
</template>
<style scoped>
.icon.liked {
  fill: var(--accent);
  color: var(--accent);
}
.member-page {
  height: 100%;
  overflow: auto;
  padding: 24px 28px 40px;
  min-width: 0;
  box-sizing: border-box;
}
.member-heading,
.member-section-title {
  display: flex;
  align-items: center;
  gap: 12px;
}
.member-heading h1 {
  margin: 0;
  font-size: 25px;
}
.member-tabs,
.member-dates {
  display: flex;
  gap: 18px;
  overflow: auto;
  padding: 16px 0;
}
.member-tabs button,
.member-dates button {
  border: 0;
  background: transparent;
  color: inherit;
  white-space: nowrap;
  padding: 6px 0;
  cursor: pointer;
}
.member-tabs [aria-pressed='true'],
.member-dates [aria-pressed='true'] {
  color: var(--accent);
  border-bottom: 2px solid var(--accent);
  font-weight: 600;
}
.member-hero {
  display: grid;
  grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
  gap: 20px;
  align-items: stretch;
}
.member-rights-card {
  min-height: 140px;
  border-radius: 10px;
  background: linear-gradient(115deg, #302b2b, #19181a);
  color: #ead2b2;
  padding: 18px;
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.member-brand {
  font-size: 13px;
  letter-spacing: 1px;
}
.member-rights-card strong {
  font-size: 20px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.member-rights-card strong span {
  font-size: 12px;
}
.member-rights-card small {
  font-size: 11px;
}
.member-rights-refresh {
  align-self: flex-start;
  margin-top: auto;
  border: 0;
  border-radius: 20px;
  background: #edd5b5;
  color: #3b2c25;
  padding: 6px 16px;
  font-size: 12px;
  cursor: pointer;
}
.member-history-intro {
  padding: 20px;
  background: var(--surface-alt);
  border-radius: 10px;
}
.member-history-intro strong {
  font-size: 20px;
}
.member-entitlements {
  display: grid;
  grid-template-columns: repeat(5, minmax(0, 1fr));
  gap: 12px;
  margin-top: 16px;
}
.member-entitlements > div {
  display: flex;
  flex-direction: column;
  gap: 4px;
  font-size: 11px;
  padding: 10px 6px;
  border-bottom: 2px solid transparent;
}
.member-entitlements [data-active='true'] {
  border-color: var(--gold);
}
.member-entitlements strong {
  font-size: 12px;
}
.member-entitlements small {
  opacity: 0.6;
}
.member-quality-note {
  font-size: 11px;
  opacity: 0.65;
  margin: 12px 0;
}
.member-error {
  color: var(--danger);
  padding: 12px;
  background: var(--danger-bg);
  border-radius: 8px;
}
.member-messages {
  display: flex;
  gap: 18px;
  font-size: 12px;
  opacity: 0.7;
  flex-wrap: wrap;
  margin: 16px 0;
}
.member-module {
  margin: 18px 0;
}
.member-section-title h2 {
  font-size: 19px;
  margin: 8px 0 16px;
}
.member-tracks {
  display: grid;
  grid-auto-flow: column;
  grid-template-rows: repeat(3, auto);
  grid-auto-columns: minmax(240px, 1fr);
  overflow-x: auto;
  gap: 6px 20px;
}
.member-track {
  display: flex;
  align-items: center;
  min-width: 0;
  padding: 6px 4px;
  border-radius: 8px;
}
.member-track:hover,
.member-track.current {
  background: var(--surface-alt);
}
.member-track-play {
  display: flex;
  align-items: center;
  gap: 10px;
  min-width: 0;
  flex: 1;
  background: transparent;
  border: 0;
  color: inherit;
  text-align: left;
  cursor: pointer;
  padding: 0;
}
.member-track-play img {
  width: 44px;
  height: 44px;
  border-radius: 8px;
}
.member-track-play > span {
  display: flex;
  flex-direction: column;
  min-width: 0;
  gap: 4px;
}
.member-track-play strong {
  font-size: 13px;
  font-weight: 500;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.member-track-play small {
  font-size: 11px;
  opacity: 0.65;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.member-track > .icon-button {
  width: 25px;
  height: 28px;
}
.member-track > .icon-button svg {
  width: 16px;
  height: 16px;
}
.member-ranks {
  display: flex;
  gap: 16px;
  overflow-x: auto;
  padding-bottom: 8px;
}
.member-rank {
  flex: 0 0 285px;
  background: var(--surface-alt);
  padding: 14px;
  border-radius: 10px;
}
.member-rank header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 8px;
}
.member-rank header > .text-button {
  font-size: 17px;
  color: inherit;
  font-weight: 600;
}
.member-rank-track {
  display: flex;
  gap: 10px;
  align-items: center;
  width: 100%;
  border: 0;
  background: transparent;
  color: inherit;
  text-align: left;
  padding: 8px 0;
  cursor: pointer;
}
.member-rank-track img {
  width: 40px;
  height: 40px;
  border-radius: 6px;
}
.member-rank-track > span:last-child {
  display: flex;
  flex-direction: column;
  gap: 4px;
  overflow: hidden;
}
.member-rank-track strong {
  font-size: 12px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.member-rank-track small {
  font-size: 10px;
  opacity: 0.6;
}
.member-history-actions {
  display: flex;
  align-items: center;
  gap: 14px;
  margin: 10px 0;
}
.member-history-actions button {
  display: flex;
  align-items: center;
  gap: 6px;
}
.member-history-actions span {
  font-size: 12px;
  opacity: 0.7;
}
.member-page :deep(.track-table) {
  height: 400px;
  min-height: 220px;
}
.member-page :deep(.track-scroll) {
  height: calc(100% - 36px);
}
@media (max-width: 900px) {
  .member-page {
    padding: 18px 16px 32px;
  }
  .member-hero {
    gap: 12px;
  }
  .member-rights-card {
    min-height: 132px;
    padding: 14px;
  }
  .member-entitlements {
    gap: 4px;
  }
  .member-entitlements strong {
    font-size: 11px;
  }
  .member-entitlements small {
    font-size: 10px;
  }
}
</style>
