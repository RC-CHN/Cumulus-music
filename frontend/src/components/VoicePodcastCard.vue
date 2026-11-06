<script setup lang="ts">
import { computed } from 'vue';
import Icon from './Icon.vue';
import { useCoverImage } from '../composables/useCoverImage';
import type { PodcastSearchFollow } from '../services/podcast';
import type { VoicePlaybackPodcast, VoicePodcastTarget } from '../services/voiceDetail';

const props = defineProps<{
  radio: VoicePlaybackPodcast;
  target: VoicePodcastTarget;
  episodeCover?: string | null;
  signedIn: boolean;
  control?: PodcastSearchFollow;
  refreshing: boolean;
  status: string;
}>();
const emit = defineEmits<{
  open: [target: VoicePodcastTarget];
  activate: [
    target: VoicePodcastTarget,
    expected: PodcastSearchFollow | undefined,
    opener: HTMLElement,
  ];
}>();
function activate(event: MouseEvent) {
  if (event.currentTarget instanceof HTMLElement)
    emit('activate', props.target, props.control, event.currentTarget);
}
const image = useCoverImage(() => ({
  owner: props.target.epoch,
  source: props.radio.coverUrl ?? props.episodeCover,
}));
const subscribed = computed(() => props.control?.kind === 'toggle' && props.control.subscribed);
const label = computed(() =>
  props.control?.kind === 'reconcile'
    ? '核对收藏'
    : subscribed.value
      ? '已收藏'
      : !props.signedIn || props.radio.ownedByViewer || props.control
        ? '收藏播单'
        : props.refreshing
          ? '读取收藏状态…'
          : '重试收藏状态',
);
</script>

<template>
  <section class="voice-podcast-card" aria-label="所属播单" data-non-selectable>
    <div class="voice-podcast-row">
      <button
        type="button"
        class="voice-podcast-open"
        data-voice-podcast-open
        :disabled="!signedIn"
        :aria-label="`查看播单：${radio.name}`"
        @click="emit('open', target)"
      >
        <span class="voice-podcast-cover" :data-cover-state="image.state.value">
          <img
            v-if="image.showImage.value"
            v-show="image.loaded.value"
            :key="image.generation.value"
            :src="image.source.value!"
            :data-cover-generation="image.generation.value"
            alt=""
            referrerpolicy="no-referrer"
            @load="image.load"
            @error="image.error"
          />
          <Icon v-if="!image.loaded.value" name="music" />
        </span>
        <span class="voice-podcast-info">
          <span class="voice-podcast-name" :title="radio.name">{{ radio.name }}</span>
          <small v-if="radio.subscriberCount !== undefined"
            >{{ radio.subscriberCount }} 人收藏</small
          >
        </span>
      </button>
      <button
        type="button"
        class="secondary voice-podcast-subscribe"
        data-voice-podcast-subscribe
        :disabled="radio.ownedByViewer || refreshing || control?.busy"
        :aria-pressed="control?.kind === 'toggle' ? subscribed : undefined"
        :title="radio.ownedByViewer ? '不能收藏自己的播单' : undefined"
        @click="activate"
      >
        <Icon v-if="control?.kind === 'toggle' && !subscribed" name="collect" />{{ label }}
      </button>
    </div>
    <p v-if="status" data-voice-podcast-status role="status">{{ status }}</p>
  </section>
</template>

<style scoped>
.voice-podcast-card {
  border-radius: 12px;
  padding: 14px 16px;
  background: color-mix(in srgb, var(--text) 7%, transparent);
}
.voice-podcast-card:focus-within,
.voice-podcast-card:hover {
  background: color-mix(in srgb, var(--text) 10%, transparent);
}
.voice-podcast-row,
.voice-podcast-open {
  display: flex;
  align-items: center;
  gap: 12px;
  min-width: 0;
}
.voice-podcast-open {
  flex: 1;
  padding: 0;
  text-align: start;
}
.voice-podcast-cover {
  width: 58px;
  height: 58px;
  flex: none;
  display: grid;
  place-items: center;
  overflow: hidden;
  border-radius: 8px;
  background: var(--surface-alt);
  color: var(--muted);
}
.voice-podcast-cover img {
  width: 100%;
  height: 100%;
  object-fit: cover;
}
.voice-podcast-info {
  display: grid;
  gap: 6px;
  min-width: 0;
}
.voice-podcast-name {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 16px;
}
.voice-podcast-info small,
.voice-podcast-card > p {
  margin: 0;
  font-size: 12px;
  color: var(--text-secondary);
}
.voice-podcast-card > p {
  margin-block-start: 8px;
}
.voice-podcast-subscribe {
  flex: none;
  display: inline-flex;
  align-items: center;
  gap: 5px;
  min-height: 34px;
  padding: 0 9px;
  font-size: 12px;
}
.voice-podcast-subscribe .icon {
  width: 15px;
  height: 15px;
}
</style>
