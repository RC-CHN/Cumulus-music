<script setup lang="ts">
import { nextTick, ref } from 'vue';
import type { useColdOffline } from '../../composables/useColdOffline';
import { usePageEntryFocus } from '../../composables/usePageEntryFocus';
import { qualityLabel } from '../../services/playbackQuality';

const props = defineProps<{ model: ReturnType<typeof useColdOffline> }>();
defineEmits<{ retry: [] }>();
const root = ref<HTMLElement>();
const heading = ref<HTMLElement>();
usePageEntryFocus(
  root,
  () => true,
  () => !document.querySelector('dialog[open],[aria-modal="true"]'),
);
function focusHeading() {
  if (root.value?.contains(document.activeElement)) {
    void nextTick(() => heading.value?.focus({ preventScroll: true }));
  }
}
function open() {
  focusHeading();
  void props.model.open();
}
function leave() {
  focusHeading();
  props.model.leave();
}
</script>

<template>
  <section ref="root" class="cold-offline-page" aria-labelledby="cold-offline-heading">
    <h1 id="cold-offline-heading" ref="heading" tabindex="-1">离线下载</h1>
    <p v-if="!model.page.value">
      暂时无法验证账号。你可以在此设备上查看和播放符合条件的已下载歌曲。
    </p>
    <div v-if="model.closeFailed.value" class="cold-offline-actions" role="alert">
      <p>无法确认离线模式已退出，请重试。</p>
      <button class="text-button" data-cold-offline-close @click="leave">重试退出</button>
    </div>
    <p v-if="model.error.value" role="alert">{{ model.error.value }}</p>
    <button
      v-if="!model.page.value && !model.closeFailed.value"
      class="primary"
      data-cold-offline-open
      :disabled="model.busy.value"
      @click="open"
    >
      查看离线下载
    </button>
    <button
      v-if="!model.page.value && !model.closeFailed.value"
      class="text-button"
      :disabled="model.busy.value"
      @click="$emit('retry')"
    >
      重新连接
    </button>
    <div v-if="model.page.value" data-cold-offline-panel>
      <div class="cold-offline-actions">
        <p role="status">
          {{
            model.playback.value?.phase === 'playing'
              ? model.currentTitle.value
                ? '正在播放：' + model.currentTitle.value
                : '正在播放离线歌曲'
              : '未播放'
          }}
        </p>
        <button
          class="text-button"
          data-cold-offline-stop
          :disabled="model.busy.value || model.playback.value?.phase !== 'playing'"
          @click="model.stop"
        >
          停止播放
        </button>
        <button class="text-button" data-cold-offline-close @click="leave">退出离线模式</button>
      </div>
      <ul v-if="model.page.value.items.length" class="cold-offline-list" aria-label="离线下载歌曲">
        <li v-for="song in model.page.value.items" :key="song.id">
          <div>
            <strong>{{ song.title }}</strong>
            <span>{{ song.artist }} · {{ qualityLabel(song.actualQuality) }}</span>
          </div>
          <button
            class="text-button"
            :data-cold-offline-play="song.id"
            :aria-label="'播放离线歌曲 ' + song.title"
            :disabled="model.busy.value || model.playback.value?.playingId === song.id"
            @click="model.play(song.id)"
          >
            播放
          </button>
        </li>
      </ul>
      <p v-else>此页没有可播放的歌曲。</p>
      <button
        v-if="model.page.value.hasMore"
        class="text-button"
        data-cold-offline-next
        :disabled="model.busy.value"
        @click="model.next"
      >
        下一页
      </button>
    </div>
  </section>
</template>

<style scoped>
.cold-offline-page {
  max-width: 820px;
  padding: 28px clamp(16px, 4vw, 42px);
}
.cold-offline-page p,
.cold-offline-list span {
  color: var(--muted);
}
.cold-offline-actions {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 12px;
}
.cold-offline-actions p {
  min-width: 0;
  overflow-wrap: anywhere;
}
.cold-offline-list {
  padding: 0;
  list-style: none;
}
.cold-offline-list li {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding: 8px 0;
  border-bottom: 1px solid var(--line);
}
.cold-offline-list li div {
  min-width: 0;
  overflow-wrap: anywhere;
}
.cold-offline-list span {
  display: block;
}
.cold-offline-list button {
  flex: none;
}
</style>
