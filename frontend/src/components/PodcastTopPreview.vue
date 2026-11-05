<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import type { PodcastPreview, PodcastTopModel } from '../composables/usePodcastTop';
import type { EpisodeView, PodcastRankResource, PodcastRankTarget } from '../services/podcast';
import Icon from './Icon.vue';

const props = defineProps<{
  model: PodcastTopModel;
  playable: boolean;
  currentVoice?: string;
  playing: boolean;
}>();
const emit = defineEmits<{
  all: [target: PodcastRankTarget];
  radio: [id: string];
  play: [voice: EpisodeView];
}>();
const root = ref<HTMLElement>();
const width = ref(0);
let resize: ResizeObserver | undefined;
const { layout, ordinary, book, canRecommend, previews, targets } = props.model;
const wideClassic = computed(
  () => layout.value === 'classic' && Math.floor((width.value + 20) / 198) >= 6,
);
watch(wideClassic, (wide) => {
  if (wide) {
    void props.model.read('score');
    void props.model.read('hot');
  }
});
const panels = computed<
  { key: string; selected: PodcastPreview; tabs: { id: PodcastPreview; name: string }[] }[]
>(() => {
  if (layout.value === 'updated')
    return [
      {
        key: 'podcast',
        selected: ordinary.value,
        tabs: [
          { id: 'score', name: '高分播客' },
          { id: 'hot', name: '最热播客' },
        ],
      },
      {
        key: 'book',
        selected: book.value,
        tabs: [
          ...(canRecommend.value ? [{ id: 'recommended' as const, name: '好书推荐' }] : []),
          { id: 'free', name: '免费听书' },
        ],
      },
    ];
  const score = { id: 'score' as const, name: '高分必听' },
    hot = { id: 'hot' as const, name: '今日最热' };
  return wideClassic.value
    ? [
        { key: 'podcast', selected: 'score', tabs: [score] },
        { key: 'hot', selected: 'hot', tabs: [hot] },
      ]
    : [{ key: 'podcast', selected: ordinary.value, tabs: [score, hot] }];
});
onMounted(() => {
  void props.model.activate();
  if (!root.value) return;
  width.value = root.value.clientWidth;
  resize = new ResizeObserver(() => {
    width.value = root.value?.clientWidth ?? 0;
  });
  resize.observe(root.value);
});
onBeforeUnmount(() => {
  resize?.disconnect();
  props.model.hide();
});
function name(resource: PodcastRankResource) {
  return resource.kind === 'radio'
    ? resource.radio.name
    : resource.kind === 'voice'
      ? resource.voice.name
      : resource.name;
}
function artwork(resource: PodcastRankResource) {
  return resource.kind === 'radio'
    ? resource.radio.coverUrl
    : resource.kind === 'voice'
      ? resource.voice.coverUrl
      : resource.avatarUrl;
}
function inactive(resource: PodcastRankResource) {
  return resource.kind === 'voice'
    ? !props.playable || resource.voice.unavailable
    : resource.kind !== 'radio' || resource.radio.unavailable;
}
function activate(resource: PodcastRankResource) {
  if (resource.kind === 'radio') emit('radio', resource.radio.id);
  else if (resource.kind === 'voice') emit('play', resource.voice);
}
function voiceIcon(resource: PodcastRankResource) {
  return resource.kind === 'voice' && resource.voice.id === props.currentVoice && props.playing
    ? 'pause'
    : 'play';
}
</script>

<template>
  <div ref="root" class="podcast-top" :data-layout="layout">
    <template v-if="layout">
      <slot v-if="layout === 'classic'" name="scene-fm" />
      <section
        v-for="panel in panels"
        :key="panel.key"
        class="podcast-top-panel"
        :data-preview="panel.key"
        :data-active-chart="panel.selected"
      >
        <header class="podcast-top-header">
          <div class="podcast-top-tabs">
            <template v-for="tab in panel.tabs" :key="tab.id">
              <span v-if="panel.tabs.length === 1" class="active">{{ tab.name }}</span>
              <button
                v-else
                type="button"
                :class="{ active: tab.id === panel.selected }"
                :aria-pressed="tab.id === panel.selected"
                @click="model.select(tab.id)"
              >
                {{ tab.name }}
              </button>
            </template>
          </div>
          <button
            type="button"
            class="text-button podcast-top-more"
            @click="emit('all', targets[panel.selected])"
          >
            查看全部<Icon name="forward" />
          </button>
        </header>
        <div v-if="previews[panel.selected]?.error" class="preview-feedback" role="alert">
          <span>{{ previews[panel.selected]?.error }}</span>
          <button class="text-button" type="button" @click="model.read(panel.selected, true)">
            重试
          </button>
        </div>
        <p v-else-if="!previews[panel.selected]?.rows" class="preview-feedback" role="status">
          正在读取榜单…
        </p>
        <p v-else-if="!previews[panel.selected]?.rows?.length" class="preview-feedback">
          暂无榜单内容
        </p>
        <div v-else class="podcast-top-items">
          <button
            v-for="(row, index) in previews[panel.selected]?.rows"
            :key="index"
            type="button"
            class="podcast-top-item"
            :title="name(row.resource)"
            :disabled="inactive(row.resource)"
            :aria-label="
              row.resource.kind === 'voice'
                ? `${voiceIcon(row.resource) === 'pause' ? '暂停' : '播放'} ${name(row.resource)}`
                : `打开 ${name(row.resource)}`
            "
            @click="activate(row.resource)"
          >
            <span class="preview-art">
              <img
                v-if="artwork(row.resource)"
                :src="artwork(row.resource)!"
                alt=""
                loading="lazy"
                referrerpolicy="no-referrer"
              />
              <Icon
                v-else
                :name="row.resource.kind === 'voice' ? voiceIcon(row.resource) : 'headphones'"
              />
              <small v-if="row.resource.kind !== 'voice'">{{ row.rank ?? index + 1 }}</small>
            </span>
            <span class="preview-name">{{ name(row.resource) }}</span>
          </button>
        </div>
      </section>
    </template>
  </div>
</template>

<style scoped>
.podcast-top[data-layout] {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(min(100%, 220px), 1fr));
  gap: 14px;
  margin: 0 0 18px;
}
.podcast-top-panel {
  min-width: 0;
  border: 1px solid var(--line);
  border-radius: 12px;
  padding: 12px;
  background: var(--surface-raised);
}
.podcast-top-header {
  display: flex;
  align-items: baseline;
  flex-wrap: wrap;
  justify-content: space-between;
  gap: 4px 12px;
}
.podcast-top-tabs {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  color: var(--muted);
  font-size: 14px;
}
.podcast-top-tabs button,
.podcast-top-more {
  border: 0;
  padding: 0;
}
.podcast-top-tabs .active {
  color: var(--text);
  font-weight: 600;
}
.podcast-top-more {
  white-space: nowrap;
  font-size: 12px;
}
.podcast-top-more .icon {
  width: 14px;
  height: 14px;
}
.podcast-top-items {
  display: grid;
  grid-template-columns: repeat(4, minmax(0, 1fr));
  gap: 8px;
  margin-top: 10px;
}
.podcast-top-item {
  min-width: 0;
  padding: 0;
  border: 0;
  display: flex;
  flex-direction: column;
  align-items: start;
  gap: 4px;
  text-align: left;
}
.preview-art {
  position: relative;
  display: grid;
  place-items: center;
  width: 100%;
  max-width: 76px;
  aspect-ratio: 1;
  border-radius: 8px;
  background: var(--surface-hover);
}
.preview-art img {
  width: 100%;
  height: 100%;
  border-radius: inherit;
  object-fit: cover;
}
.preview-art small {
  position: absolute;
  left: 4px;
  bottom: 4px;
  color: white;
  background: #333b;
  border-radius: 4px;
  padding: 1px 4px;
}
.preview-name {
  max-width: 100%;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 12px;
}
.preview-feedback {
  display: flex;
  align-items: center;
  gap: 6px;
  color: var(--muted);
  font-size: 13px;
}
</style>
