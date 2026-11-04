<script setup lang="ts">
import { onMounted, ref } from 'vue';
import type { PodcastHomeModel } from '../composables/usePodcastHome';
import type { PodcastCatalogueModel } from '../composables/usePodcastCatalogue';
import { usePageEntryFocus } from '../composables/usePageEntryFocus';
import type { EpisodeView } from '../services/podcast';
import { time } from '../services/player';
import Icon from './Icon.vue';
import PodcastCategoryStrip from './PodcastCategoryStrip.vue';

const props = defineProps<{
  model: PodcastHomeModel;
  catalogue: PodcastCatalogueModel;
  playable: boolean;
  currentVoice?: string;
  playing: boolean;
}>();
const emit = defineEmits<{
  open: [id: string];
  play: [voice: EpisodeView];
  settings: [];
  category: [id: string];
}>();
const root = ref<HTMLElement>();
const { blocks, enabled, pending, error, continuation, restart } = props.model;
usePageEntryFocus(
  root,
  () => !pending.value,
  () => true,
);
onMounted(() => {
  props.model.activate();
  void props.catalogue.loadCategories();
});
function scrolled() {
  const pane = root.value;
  if (
    pane &&
    pane.scrollTop > 0 &&
    pane.scrollHeight - pane.clientHeight - pane.scrollTop < 240 &&
    !error.value
  )
    void props.model.load();
}
</script>

<template>
  <section
    ref="root"
    class="podcast-page podcast-discovery"
    aria-label="播客发现"
    @scroll.passive="scrolled"
  >
    <header class="podcast-heading">
      <h1>播客</h1>
      <button
        class="text-button"
        type="button"
        aria-label="刷新播客发现"
        :disabled="pending"
        @click="model.load(true)"
      >
        <Icon name="refresh" />刷新
      </button>
    </header>
    <PodcastCategoryStrip :model="catalogue" @open="(id) => emit('category', id)" />
    <div v-if="enabled === false" class="empty-state">
      <p>个性化推荐已关闭</p>
      <button class="secondary" type="button" @click="emit('settings')">前往设置</button>
    </div>
    <section
      v-for="(block, index) in blocks"
      :key="index"
      class="podcast-home-block"
      :data-block-kind="block.kind"
    >
      <h2 v-if="block.title">{{ block.title }}</h2>
      <div v-if="block.kind === 'radios'" class="podcast-home-cards">
        <button
          v-for="(card, at) in block.cards"
          :key="at"
          class="podcast-card"
          :data-home-radio="card.id"
          :disabled="card.unavailable"
          @click="emit('open', card.id)"
        >
          <img
            v-if="card.coverUrl"
            :src="card.coverUrl"
            alt=""
            loading="lazy"
            referrerpolicy="no-referrer"
          />
          <div v-else class="podcast-art"><Icon name="headphones" /></div>
          <strong :title="card.name">{{ card.name }}</strong>
          <span>{{ card.creator || '播客' }} · {{ card.programCount }} 集</span>
        </button>
      </div>
      <div v-else class="podcast-home-voices">
        <div v-for="(group, g) in block.groups" :key="g" class="podcast-home-group">
          <article
            v-for="(voice, v) in group"
            :key="v"
            class="podcast-home-voice"
            :data-home-voice="voice.id"
          >
            <button
              type="button"
              class="icon-button"
              :disabled="!playable || voice.unavailable"
              :aria-label="`${currentVoice === voice.id && playing ? '暂停' : '播放'} ${voice.name}`"
              @click="emit('play', voice)"
            >
              <Icon :name="currentVoice === voice.id && playing ? 'pause' : 'play'" />
            </button>
            <div class="podcast-home-voice-copy">
              <strong :title="voice.name">{{ voice.name }}</strong>
              <button
                v-if="voice.radioId"
                type="button"
                class="text-button"
                @click="emit('open', voice.radioId)"
              >
                {{ voice.radioName || '打开所属播客' }}
              </button>
              <small v-else>{{ voice.creator }}</small>
            </div>
            <span class="muted" v-if="voice.durationMs">{{ time(voice.durationMs / 1000) }}</span>
          </article>
        </div>
      </div>
    </section>
    <p
      v-if="enabled === true && !blocks.length && !continuation && !pending && !error"
      class="empty-state"
    >
      暂无播客推荐
    </p>
    <footer class="search-load-more">
      <span v-if="pending" role="status">正在读取播客推荐…</span>
      <span v-else-if="error" role="alert">{{ error }}</span>
      <button
        v-if="error || continuation"
        type="button"
        class="text-button"
        :aria-disabled="pending"
        @click="model.load()"
      >
        {{ restart ? '重新读取播客推荐' : error ? '重试' : '加载更多播客' }}
      </button>
    </footer>
  </section>
</template>

<style scoped>
.podcast-discovery .podcast-heading {
  align-items: baseline;
}
.podcast-home-block {
  margin: 0 0 24px;
  content-visibility: auto;
  contain-intrinsic-height: auto 180px;
}
.podcast-home-block h2 {
  font-size: 20px;
  margin: 0 0 14px;
}
.podcast-home-cards {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(max(112px, calc((100% - 80px) / 6)), 1fr));
  gap: 16px;
}
.podcast-home-cards .podcast-card span,
.podcast-home-voice-copy strong,
.podcast-home-voice-copy button {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.podcast-home-voices {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(min(100%, 246px), 1fr));
  gap: 12px;
}
.podcast-home-group {
  min-width: 0;
}
.podcast-home-voice {
  display: flex;
  align-items: center;
  gap: 8px;
  min-height: 56px;
  padding: 6px 8px;
  border-radius: 8px;
  background: var(--surface-soft);
}
.podcast-home-voice-copy {
  display: flex;
  flex-direction: column;
  flex: 1;
  align-items: start;
  min-width: 0;
}
.podcast-home-voice-copy strong,
.podcast-home-voice-copy button {
  max-width: 100%;
}
.podcast-home-voice-copy button {
  padding: 0;
  color: var(--muted);
}
</style>
