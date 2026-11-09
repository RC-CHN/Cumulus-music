<script setup lang="ts">
import { computed, nextTick, onMounted, ref, watch } from 'vue';
import { usePageEntryFocus } from '../composables/usePageEntryFocus';
import Icon from './Icon.vue';
import { time } from '../services/player';
import type { PodcastModel } from '../composables/usePodcasts';
import type { PodcastRecentModel } from '../composables/usePodcastRecent';
import PodcastRecentList from './PodcastRecentList.vue';
import PodcastProgramMenu from './PodcastProgramMenu.vue';
import PodcastSubscriptionRecommendations from './PodcastSubscriptionRecommendations.vue';
import AudiobookSkipMenu from './AudiobookSkipMenu.vue';
import { audiobookSkipSaved, type AudiobookSkipContext } from '../services/audiobookSkip';
import type {
  PodcastScope,
  EpisodeView,
  PodcastRecommendationAction,
  PodcastProgramTarget,
} from '../services/podcast';
const props = defineProps<{
  model: PodcastModel;
  recent: PodcastRecentModel;
  playable: boolean;
  queueable: boolean;
  queueProgram: (epoch: string, program: PodcastProgramTarget) => Promise<boolean>;
  currentId?: string;
  currentRadioId?: string;
  playing: boolean;
}>();
const emit = defineEmits<{
  play: [scope: PodcastScope, revision: string, id: string | null];
  recentPlay: [revision: string, id: string, doubleClick: boolean];
  recommendedPlay: [id: string, action: PodcastRecommendationAction];
  comments: [id: string, title: string];
  download: [voices: EpisodeView[]];
}>();
const { page, query, loading, busy, error, notice, operations, recentSelected } = props.model;
const root = ref<HTMLElement>();
const recentList = ref<InstanceType<typeof PodcastRecentList>>();
usePageEntryFocus(
  root,
  () =>
    recentSelected.value
      ? !!props.recent.page.value || !!props.recent.issue.value
      : !!page.value || !!error.value,
  () => true,
);
const keyword = ref(query.value.keyword);
const reference = ref('');
watch(
  () => query.value.keyword,
  (v) => (keyword.value = v),
);
const scope = computed(() => query.value.scope);
const detail = computed(() => !recentSelected.value && scope.value.kind === 'radio');
const menuPage = computed(() => {
  const displayed = page.value,
    requested = scope.value;
  return detail.value &&
    !loading.value &&
    displayed?.epoch === props.model.epoch.value &&
    displayed.scope.kind === 'radio' &&
    requested.kind === 'radio' &&
    displayed.scope.id === requested.id &&
    displayed.scope.ascending === requested.ascending &&
    displayed.revision === query.value.revision &&
    displayed.offset === query.value.offset
    ? displayed
    : undefined;
});
const programMenu = ref<InstanceType<typeof PodcastProgramMenu>>();
function openProgramMenu(event: MouseEvent | KeyboardEvent, voice: EpisodeView) {
  if (!menuPage.value || !(event.currentTarget instanceof HTMLElement)) return;
  const current = event.currentTarget;
  const anchor =
    current instanceof HTMLButtonElement
      ? current
      : current.querySelector<HTMLButtonElement>('[data-podcast-program-more]');
  if (!anchor) return;
  const point =
    event instanceof MouseEvent && event.button === 2
      ? { x: event.clientX, y: event.clientY }
      : undefined;
  void programMenu.value?.open(voice, anchor, point);
}
function programContext(event: MouseEvent, voice: EpisodeView) {
  if (!menuPage.value) return;
  event.preventDefault();
  event.stopPropagation();
  openProgramMenu(event, voice);
}
function programKeyboard(event: KeyboardEvent, voice: EpisodeView) {
  if (!menuPage.value || (event.key !== 'ContextMenu' && !(event.shiftKey && event.key === 'F10')))
    return;
  event.preventDefault();
  event.stopPropagation();
  openProgramMenu(event, voice);
}

const audiobookSkipContext = computed<AudiobookSkipContext | undefined>(() => {
  const current = page.value;
  if (
    !detail.value ||
    scope.value.kind !== 'radio' ||
    !current ||
    current.scope.kind !== 'radio' ||
    current.scope.id !== scope.value.id ||
    current.scope.ascending !== scope.value.ascending ||
    typeof current.audiobookSkipEnabled !== 'boolean'
  )
    return undefined;
  return { kind: 'detail', scope: current.scope, revision: current.revision };
});
watch(audiobookSkipSaved, (saved) => {
  const current = page.value;
  if (
    current &&
    saved &&
    current.epoch === saved.epoch &&
    current.scope.kind === 'radio' &&
    current.scope.id === saved.value.radioId &&
    typeof current.audiobookSkipEnabled === 'boolean'
  ) {
    page.value = { ...current, audiobookSkipEnabled: saved.value.setting.kind !== 'close' };
  }
});
async function restoreRecentScroll() {
  if (!recentSelected.value) return;
  await nextTick();
  const pane = root.value;
  if (!pane || !recentSelected.value) return;
  pane.scrollTop = props.recent.scrollTop.value;
  const origin = props.recent.radioOpener.value;
  if (origin) {
    await recentList.value?.focusRadio(origin);
    if (props.recent.radioOpener.value === origin) props.recent.radioOpener.value = undefined;
  }
}
function openRecentRadio(id: string, voiceId: string) {
  if (root.value) props.recent.scrollTop.value = root.value.scrollTop;
  props.recent.radioOpener.value = voiceId;
  props.model.navigate({ kind: 'radio', id, ascending: true });
}
onMounted(restoreRecentScroll);
watch(recentSelected, restoreRecentScroll);
function scrolled() {
  const pane = root.value;
  if (!pane || !recentSelected.value) return;
  props.recent.scrollTop.value = pane.scrollTop;
  if (pane.scrollTop > 0 && pane.scrollHeight - pane.clientHeight - pane.scrollTop < 200)
    props.recent.next();
}
async function refresh() {
  if (!recentSelected.value) {
    void props.model.load(true);
    return;
  }
  await props.recent.refresh();
  await restoreRecentScroll();
}
const tabs = [
  { kind: 'podcasts', label: '我的收藏' },
  { kind: 'created', label: '我的创建' },
  { kind: 'purchased', label: '我的购买' },
  { kind: 'liked', label: '点赞的单集' },
] as const;
function tab(value: PodcastScope) {
  keyword.value = '';
  props.model.navigate(value);
}
function play(id: string | null = null) {
  if (page.value) emit('play', scope.value, page.value.revision, id);
}
const pending = props.model.pending;
</script>
<template>
  <section ref="root" class="podcast-page" aria-label="我的播客" @scroll.passive="scrolled">
    <header class="podcast-heading">
      <h1>{{ detail ? '播客详情' : '我的播客' }}</h1>
      <button
        class="text-button"
        :disabled="recentSelected ? recent.pending.value : loading || busy"
        @click="refresh"
      >
        <Icon name="refresh" />刷新
      </button>
    </header>
    <form
      v-if="!detail && !recentSelected"
      class="podcast-open"
      @submit.prevent="model.open(reference)"
    >
      <input
        v-model="reference"
        aria-label="播客编号或链接"
        placeholder="播客编号或链接"
        maxlength="2048"
        :disabled="busy"
      />
      <button class="text-button" :disabled="busy || !reference.trim()">打开播客</button>
    </form>
    <button v-if="detail && model.canBack.value" class="text-button" @click="model.back">
      <Icon name="back" />返回播客列表
    </button>
    <nav v-if="!detail" class="podcast-tabs" aria-label="播客分类">
      <button
        v-for="t in tabs"
        :key="t.kind"
        :class="{
          active:
            !recentSelected &&
            (scope.kind === t.kind || (t.kind === 'podcasts' && scope.kind === 'audiobooks')),
        }"
        @click="tab({ kind: t.kind })"
      >
        {{ t.label }}
      </button>
      <button
        type="button"
        data-podcast-recent-tab
        :class="{ active: recentSelected }"
        :aria-current="recentSelected ? 'page' : undefined"
        :disabled="!model.epoch.value"
        @click="model.showRecent"
      >
        最近更新单集
      </button>
    </nav>
    <PodcastRecentList
      v-if="recentSelected"
      ref="recentList"
      :model="recent"
      :playable="playable"
      :current-id="currentId"
      :playing="playing"
      @play="(revision, id, doubleClick) => emit('recentPlay', revision, id, doubleClick)"
      @radio="openRecentRadio"
    />
    <template v-else>
      <div v-if="['podcasts', 'audiobooks'].includes(scope.kind)" class="podcast-subtabs">
        <button :class="{ active: scope.kind === 'podcasts' }" @click="tab({ kind: 'podcasts' })">
          播客</button
        ><button
          :class="{ active: scope.kind === 'audiobooks' }"
          @click="tab({ kind: 'audiobooks' })"
        >
          有声书
        </button>
      </div>
      <div v-if="error" class="error-state" role="alert">
        {{ error }}
        <button class="text-button" :disabled="loading" @click="model.load(true)">重试</button>
      </div>
      <p v-if="notice" role="status" class="form-success">{{ notice }}</p>
      <div v-if="operations.length" class="podcast-pending">
        <p>以下操作尚未确认，请先核对结果。</p>
        <button
          v-for="op in operations"
          :key="op.action + op.id"
          class="secondary"
          :disabled="busy"
          @click="model.command({ kind: 'reconcile', action: op.action, id: op.id })"
        >
          核对{{ op.action === 'like' ? '单集点赞' : op.action === 'resume' ? '续播设置' : '收藏' }}
          ·
          {{ op.id }}
        </button>
      </div>
      <div v-if="page?.radio" class="podcast-cover">
        <img
          v-if="page.radio.coverUrl"
          :src="page.radio.coverUrl"
          alt=""
          referrerpolicy="no-referrer"
        />
        <div v-else class="podcast-art"><Icon name="music" /></div>
        <div class="podcast-summary">
          <h2>{{ page.radio.name }}</h2>
          <span>{{ page.radio.creator || '主播' }}</span
          ><small
            >共 {{ page.remoteTotal }} 集<span v-if="page.radio.feeType"> · 含付费内容</span></small
          >
          <p>{{ page.radio.description || '主播暂未填写简介' }}</p>
          <div class="podcast-actions">
            <button class="primary" :disabled="!playable || loading || !page.total" @click="play()">
              <Icon name="play" />播放全部</button
            ><button
              class="secondary"
              :disabled="
                busy ||
                loading ||
                page.radio.subscribed === null ||
                pending('subscribe', page.radio.id)
              "
              :aria-pressed="page.radio.subscribed ?? false"
              @click="model.change('subscribe', page.radio.id, !page.radio.subscribed)"
            >
              {{ page.radio.subscribed ? '已收藏' : '收藏播客' }}
            </button>
            <AudiobookSkipMenu
              :epoch="page.epoch"
              :context="audiobookSkipContext"
              :enabled="page.audiobookSkipEnabled === true"
              :disabled="loading || busy"
            />
            <label v-if="page.resumeEnabled !== null"
              ><input
                type="checkbox"
                :checked="page.resumeEnabled"
                :disabled="busy || loading || pending('resume', page.radio.id)"
                @change="
                  model.change(
                    'resume',
                    page!.radio!.id,
                    ($event.target as HTMLInputElement).checked,
                  )
                "
              />续播</label
            >
          </div>
        </div>
      </div>
      <div class="podcast-toolbar">
        <h2 v-if="detail">
          声音 <span class="muted">{{ page?.total ?? 0 }}</span>
        </h2>
        <span v-else class="muted"
          >{{ page?.total ?? 0 }} {{ scope.kind === 'liked' ? '集' : '个' }}</span
        >
        <button
          v-if="detail"
          class="text-button"
          :disabled="loading || busy || !page?.radio || page.radio.unavailable"
          @click="model.toggleRadioOrder"
        >
          {{ scope.kind === 'radio' && scope.ascending ? '正序' : '倒序' }}
        </button>
        <button
          v-if="scope.kind === 'liked'"
          class="primary"
          :disabled="!playable || loading || !page?.total"
          @click="play()"
        >
          播放全部
        </button>
        <form @submit.prevent="model.search(keyword)">
          <input
            v-model="keyword"
            placeholder="搜索当前列表"
            aria-label="搜索当前播客列表"
            maxlength="200"
          /><button class="text-button" type="submit">搜索</button>
        </form>
      </div>
      <div v-if="loading && !page" class="empty-state" role="status">
        <span class="spinner" />
        <p>正在读取播客…</p>
      </div>
      <div v-else-if="page && !page.total" class="empty-state">
        <Icon name="music" />
        <p>
          {{
            keyword
              ? '没有匹配的内容'
              : scope.kind === 'purchased'
                ? '还没有购买的播客'
                : scope.kind === 'audiobooks'
                  ? '还没有收藏的有声书'
                  : '这里还没有内容'
          }}
        </p>
      </div>
      <div v-if="page?.cards.length" class="podcast-grid">
        <button
          v-for="card in page.cards"
          :key="card.id"
          class="podcast-card"
          :data-radio-id="card.id"
          @click="model.openCard(card.id)"
        >
          <div class="podcast-card-cover">
            <img
              v-if="card.coverUrl"
              :src="card.coverUrl"
              alt=""
              loading="lazy"
              referrerpolicy="no-referrer"
            />
            <div v-else class="podcast-art"><Icon name="music" /></div>
            <span
              v-if="card.newEpisodeCount"
              class="podcast-new-badge"
              data-podcast-new-badge
              :aria-label="`${card.newEpisodeCount} 集新节目`"
              >{{ card.newEpisodeCount > 99 ? '99+' : card.newEpisodeCount }}</span
            >
          </div>
          <strong>{{ card.name }}</strong
          ><span>{{ card.creator || '播客' }} · {{ card.programCount }} 集</span
          ><small v-if="card.unavailable">部分内容暂不可用</small>
        </button>
      </div>
      <div v-if="page?.episodes.length" class="episode-list" aria-label="节目列表">
        <article
          v-for="(voice, i) in page.episodes"
          :key="voice.id"
          class="episode-row"
          :class="{ current: currentId === voice.id, 'with-more': detail }"
          :data-voice-id="voice.id"
          @contextmenu="programContext($event, voice)"
          @keydown="programKeyboard($event, voice)"
          @dblclick="playable && !loading && !voice.unavailable && play(voice.id)"
        >
          <button
            class="icon-button"
            :aria-label="'播放 ' + voice.name"
            :disabled="!playable || loading || voice.unavailable"
            @click="play(voice.id)"
          >
            <Icon name="play" />
          </button>
          <div class="episode-info">
            <strong :title="voice.name">{{ voice.name }}</strong
            ><small
              >{{ voice.creator || voice.radioName || '单集' }}
              <span v-if="voice.feeType"> · 付费内容</span></small
            >
          </div>
          <span class="episode-time">{{ time(voice.durationMs / 1000) }}</span>
          <button
            class="icon-button"
            :class="{ liked: voice.liked }"
            :aria-label="voice.liked ? '取消单集点赞' : '点赞单集'"
            :aria-pressed="voice.liked"
            :disabled="busy || loading || pending('like', voice.id)"
            @click="model.change('like', voice.id, !voice.liked)"
          >
            <Icon name="heart" />
          </button>
          <button
            class="icon-button"
            aria-label="下载单集"
            :disabled="loading || voice.unavailable"
            @click="emit('download', [voice])"
          >
            <Icon name="down" /></button
          ><button
            class="text-button episode-comment"
            aria-label="单集评论"
            @click="emit('comments', voice.id, voice.name)"
          >
            评论
          </button>
          <button
            v-if="detail"
            type="button"
            class="icon-button"
            data-podcast-program-more
            :aria-label="`更多：${voice.name}`"
            aria-haspopup="menu"
            aria-expanded="false"
            :disabled="!menuPage"
            @click.stop="openProgramMenu($event, voice)"
            @dblclick.stop
          >
            <Icon name="more" />
          </button>
          <span class="sr-only">第 {{ page.offset + i + 1 }} 集</span>
        </article>
      </div>
      <PodcastProgramMenu
        ref="programMenu"
        :page="menuPage"
        :playable="playable"
        :queueable="queueable"
        :queue="queueProgram"
        @play="play"
        @comments="(id, title) => emit('comments', id, title)"
        @download="(voices) => emit('download', voices)"
      />
      <footer v-if="page && page.total > query.limit" class="podcast-pagination">
        <button
          class="secondary"
          :disabled="loading || page.offset === 0"
          @click="model.paginate(Math.max(0, page.offset - query.limit))"
        >
          上一页</button
        ><span
          >{{ page.offset + 1 }}–{{ Math.min(page.offset + query.limit, page.total) }} /
          {{ page.total }}</span
        ><button
          class="secondary"
          :disabled="loading || page.offset + query.limit >= page.total"
          @click="model.paginate(page.offset + query.limit)"
        >
          下一页
        </button>
      </footer>
      <PodcastSubscriptionRecommendations
        v-if="page && model.recommendedCards.value.length"
        :cards="model.recommendedCards.value"
        :revision="page.revision"
        :can-collect="model.canCollectRecommendation"
        :disabled="busy"
        :can-play="playable"
        :current-radio-id="currentRadioId"
        :playing="playing"
        @open="model.openRecommended"
        @play="(id, action) => emit('recommendedPlay', id, action)"
        @collect="model.collectRecommended"
      />
    </template>
  </section>
</template>
