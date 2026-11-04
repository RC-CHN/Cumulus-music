<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from 'vue';
import type { PodcastRankingsModel } from '../composables/usePodcastRankings';
import { usePageEntryFocus } from '../composables/usePageEntryFocus';
import type { EpisodeView, PodcastRankRow } from '../services/podcast';
import { time } from '../services/player';
import Icon from './Icon.vue';

const props = defineProps<{
  model: PodcastRankingsModel;
  playable: boolean;
  currentVoice?: string;
  playing: boolean;
}>();
const emit = defineEmits<{ radio: [id: string]; host: [id: string]; play: [voice: EpisodeView] }>();
const root = ref<HTMLElement>();
const { groups, group, menu, tab, rows, limit, pending, error, configError } = props.model;
usePageEntryFocus(
  root,
  () => !pending.value,
  () => true,
);
onMounted(() => props.model.activate());
onBeforeUnmount(() => props.model.dropPersonalized());
const change = (row: PodcastRankRow) =>
  row.rank !== null && row.lastRank !== null ? row.lastRank - row.rank : 0;
function showMore() {
  limit.value = Math.min(rows.value.length, limit.value + 20);
}
</script>

<template>
  <section ref="root" class="podcast-page podcast-rankings" aria-label="播客排行榜">
    <header class="podcast-heading"><h1>播客排行榜</h1></header>
    <nav class="podcast-rank-groups" aria-label="排行榜类型">
      <button
        v-for="item in groups"
        :key="item.group"
        type="button"
        :aria-current="item.group === group ? 'page' : undefined"
        @click="model.open(item.group)"
      >
        {{ item.name }}
      </button>
    </nav>
    <nav v-if="menu.length" class="podcast-subtabs" aria-label="榜单">
      <button
        v-for="item in menu"
        :key="model.tabKey(item)"
        type="button"
        :class="{ active: model.tabKey(tab) === model.tabKey(item) }"
        :aria-pressed="model.tabKey(tab) === model.tabKey(item)"
        @click="model.choose(item)"
      >
        {{ item.name }}
      </button>
    </nav>
    <div v-if="configError" class="rank-feedback" role="status">
      <span>暂时无法读取听书榜配置</span
      ><button class="text-button" type="button" @click="model.bookConfig()">重试听书榜</button>
    </div>
    <p v-if="pending" role="status" class="muted">正在读取榜单…</p>
    <div v-else-if="error" class="rank-feedback" role="alert">
      <span>{{ error }}</span
      ><button class="text-button" type="button" @click="model.retry()">重试</button>
    </div>
    <p v-else-if="!rows.length" class="empty-state">暂无榜单内容</p>
    <ol v-if="rows.length" class="podcast-rank-rows">
      <li
        v-for="(row, index) in rows.slice(0, limit)"
        :key="index"
        class="podcast-rank-row"
        :data-rank="row.rank ?? index + 1"
        :data-resource="row.resource.kind"
      >
        <div class="rank-index">
          <strong>{{ row.rank ?? index + 1 }}</strong>
          <small
            v-if="tab?.showRankOffset && change(row)"
            :aria-label="`${change(row) > 0 ? '上升' : '下降'} ${Math.abs(change(row))} 名`"
            >{{ change(row) > 0 ? '↑' : '↓' }}{{ Math.abs(change(row)) }}</small
          >
        </div>
        <button
          v-if="row.resource.kind === 'radio'"
          type="button"
          class="rank-resource"
          :disabled="row.resource.radio.unavailable"
          @click="emit('radio', row.resource.radio.id)"
        >
          <img
            v-if="row.resource.radio.coverUrl"
            :src="row.resource.radio.coverUrl"
            alt=""
            loading="lazy"
            referrerpolicy="no-referrer"
          /><Icon v-else name="headphones" />
          <span
            ><strong>{{ row.resource.radio.name }}</strong
            ><small>{{ row.resource.radio.creator }}</small></span
          >
        </button>
        <template v-else-if="row.resource.kind === 'voice'">
          <button
            class="icon-button"
            type="button"
            :disabled="!playable || row.resource.voice.unavailable"
            :aria-label="`${currentVoice === row.resource.voice.id && playing ? '暂停' : '播放'} ${row.resource.voice.name}`"
            @click="emit('play', row.resource.voice)"
          >
            <Icon :name="currentVoice === row.resource.voice.id && playing ? 'pause' : 'play'" />
          </button>
          <span class="rank-episode"
            ><strong>{{ row.resource.voice.name }}</strong>
            <button
              v-if="row.resource.voice.radioId"
              class="text-button"
              type="button"
              @click="emit('radio', row.resource.voice.radioId)"
            >
              {{ row.resource.voice.radioName || '打开所属播客' }}
            </button>
            <small v-else>{{
              row.resource.voice.radioName || row.resource.voice.creator
            }}</small></span
          >
          <small v-if="row.resource.voice.durationMs">{{
            time(row.resource.voice.durationMs / 1000)
          }}</small>
        </template>
        <button v-else type="button" class="rank-resource" @click="emit('host', row.resource.id)">
          <img
            v-if="row.resource.avatarUrl"
            :src="row.resource.avatarUrl"
            alt=""
            loading="lazy"
            referrerpolicy="no-referrer"
          /><Icon v-else name="user" /><strong>{{ row.resource.name }}</strong>
        </button>
        <span v-if="tab?.showScore && row.score" class="rank-score">{{ row.score }} 分</span>
      </li>
    </ol>
    <button v-if="limit < rows.length" type="button" class="text-button" @click="showMore">
      显示更多排名
    </button>
  </section>
</template>

<style scoped>
.podcast-rank-groups,
.podcast-rankings .podcast-subtabs {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 12px 24px;
  margin: 0 0 14px;
}
.podcast-rank-groups button {
  border: 0;
  padding: 0;
  font-size: 18px;
  line-height: 25px;
}
.podcast-rank-groups [aria-current='page'] {
  color: var(--accent);
  font-weight: 600;
}
.podcast-rank-rows {
  padding: 0;
  list-style: none;
}
.podcast-rank-row {
  min-width: 0;
  min-height: 64px;
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 6px 10px;
  border-radius: 8px;
}
.podcast-rank-row:nth-child(odd) {
  background: var(--surface-soft);
}
.rank-index {
  flex: 0 0 32px;
  display: flex;
  flex-direction: column;
  text-align: center;
}
.rank-index small,
.rank-score,
.rank-episode small,
.rank-resource small {
  color: var(--muted);
}
.rank-resource {
  min-width: 0;
  display: flex;
  flex: 1;
  align-items: center;
  text-align: left;
  gap: 12px;
  border: 0;
}
.rank-resource img,
.rank-resource > .icon {
  width: 48px;
  height: 48px;
  flex: none;
  border-radius: 8px;
  object-fit: cover;
}
.rank-resource > .icon {
  padding: 12px;
  background: var(--surface-raised);
}
.rank-resource span,
.rank-episode {
  min-width: 0;
  display: flex;
  flex-direction: column;
  align-items: start;
}
.rank-episode {
  flex: 1;
}
.rank-resource strong,
.rank-resource small,
.rank-episode strong,
.rank-episode small,
.rank-episode button {
  max-width: 100%;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.rank-episode button {
  padding: 0;
}
.rank-feedback {
  display: flex;
  align-items: center;
  gap: 8px;
}
</style>
