<script setup lang="ts">
import { computed } from 'vue';
import Icon from './Icon.vue';
import type { SceneFmModel } from '../composables/useSceneFm';
import type { EpisodeView } from '../services/podcast';
const props = defineProps<{ model: SceneFmModel; playable: boolean }>();
defineEmits<{
  radio: [id: string];
  comments: [id: string, title: string];
  download: [voice: EpisodeView];
}>();
const { view, loading, busy, error } = props.model;
const category = computed(() => view.value?.scenes.find((s) => s.id === view.value?.activeId));
</script>
<template>
  <section class="scene-fm-page" aria-label="随心听FM">
    <header>
      <div>
        <h1>随心听 FM</h1>
        <p class="muted">选个喜欢的频道，让好声音陪伴你</p>
      </div>
      <button
        class="text-button"
        :disabled="loading || busy || view?.pending"
        @click="model.load(true)"
      >
        <Icon name="refresh" />刷新
      </button>
    </header>
    <div v-if="error" class="error-state" role="alert">
      {{ error
      }}<button
        class="text-button"
        :disabled="loading || busy"
        @click="view?.pending ? model.act('play') : model.load()"
      >
        重试
      </button>
    </div>
    <div v-if="loading && !view?.revision" class="empty-state" role="status">
      <span class="spinner" />
      <p>正在寻找好声音…</p>
    </div>
    <template v-else-if="view?.revision">
      <div class="scene-fm-card">
        <div class="scene-fm-top">
          <span>随心听FM</span><span>{{ category?.label }}</span>
        </div>
        <div v-if="view.episode" class="scene-fm-content">
          <button
            class="scene-fm-cover"
            :disabled="!playable || busy || loading || view.episode.unavailable"
            :aria-label="view.playing ? '暂停随心听FM' : '播放随心听FM'"
            @click="model.act('toggle')"
          >
            <img
              v-if="view.episode.coverUrl"
              :src="view.episode.coverUrl"
              alt=""
              referrerpolicy="no-referrer"
            /><Icon v-else name="music" />
            <span class="scene-fm-play"><Icon :name="view.playing ? 'pause' : 'play'" /></span>
          </button>
          <div class="scene-fm-info">
            <h2 :title="view.episode.name">{{ view.episode.name }}</h2>
            <button
              v-if="view.episode.radioId"
              class="scene-fm-radio"
              @click="$emit('radio', view.episode.radioId)"
            >
              播单：{{ view.episode.radioName || '查看播客' }}
            </button>
            <p v-else>{{ view.episode.creator }}</p>
            <span v-if="view.episode.feeType" class="scene-fm-fee">含付费内容</span>
          </div>
        </div>
        <p v-else class="scene-fm-empty">这个频道暂时没有节目，试试其他频道吧</p>
        <nav class="scene-fm-categories" aria-label="随心听频道">
          <button
            v-for="scene in view.scenes"
            :key="scene.id"
            :data-scene-id="scene.id"
            :class="{ active: scene.id === view.activeId }"
            :aria-pressed="scene.id === view.activeId"
            :disabled="busy || loading || !playable"
            @click="model.select(scene.id)"
          >
            {{ scene.name }}
          </button>
        </nav>
      </div>
      <div v-if="view.pending" class="scene-fm-recovery" role="status">
        <p>上次节目选择尚未完成，可重试播放或取消这次选择。</p>
        <button
          class="secondary"
          :disabled="busy || loading || !playable"
          @click="model.act('play')"
        >
          重试播放</button
        ><button class="text-button" :disabled="busy || loading" @click="model.act('cancel')">
          取消这次选择
        </button>
      </div>
      <div v-if="view.episode" class="scene-fm-actions">
        <button class="text-button" @click="$emit('comments', view.episode.id, view.episode.name)">
          <Icon name="comment" />单集评论</button
        ><button
          class="text-button"
          :disabled="view.episode.unavailable"
          @click="$emit('download', view.episode)"
        >
          <Icon name="down" />下载单集</button
        ><span class="muted">当前频道 {{ category?.count ?? 0 }} 集</span>
      </div>
      <label class="scene-fm-auto"
        ><input
          type="checkbox"
          :checked="view.autoplay"
          :disabled="busy || loading"
          @change="
            model.command({
              kind: 'autoplay',
              enabled: ($event.target as HTMLInputElement).checked,
            })
          "
        />进入页面时自动播放</label
      >
      <p class="muted scene-fm-hint">
        正在播放其他内容时不会自动切换。可使用底部播放条切换单集和调整播放进度。
      </p>
    </template>
  </section>
</template>
<style scoped>
.scene-fm-page {
  padding: 26px 30px;
  overflow: auto;
  height: 100%;
  box-sizing: border-box;
}
.scene-fm-page header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  margin-bottom: 22px;
}
.scene-fm-page h1 {
  font-size: 25px;
  margin: 0 0 8px;
}
.scene-fm-page header p {
  font-size: 13px;
  margin: 0;
}
.scene-fm-card {
  max-width: 670px;
  border-radius: 14px;
  background: linear-gradient(125deg, #59627f, #333950 75%, #493b55);
  color: var(--on-accent);
  padding: 20px 24px;
  box-sizing: border-box;
}
.scene-fm-top {
  display: flex;
  justify-content: space-between;
  gap: 12px;
  font-size: 12px;
  opacity: 0.85;
}
.scene-fm-content {
  display: flex;
  gap: 22px;
  align-items: center;
  padding: 24px 0;
}
.scene-fm-cover {
  padding: 0;
  position: relative;
  width: 112px;
  height: 112px;
  flex-shrink: 0;
  border: 0;
  border-radius: 10px;
  overflow: hidden;
  background: #ffffff22;
  color: var(--on-accent);
  cursor: pointer;
}
.scene-fm-cover > img {
  width: 100%;
  height: 100%;
  object-fit: cover;
}
.scene-fm-cover > svg {
  width: 38px;
  height: 38px;
}
.scene-fm-play {
  position: absolute;
  inset: 0;
  background: #0003;
  display: grid;
  place-items: center;
}
.scene-fm-play svg {
  box-sizing: content-box;
  background: #ffffffee;
  color: #384054;
  border-radius: 50%;
  padding: 12px;
  width: 24px;
  height: 24px;
}
.scene-fm-info {
  min-width: 0;
}
.scene-fm-info h2 {
  font-size: 17px;
  line-height: 1.6;
  margin: 0 0 10px;
  display: -webkit-box;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
  overflow: hidden;
}
.scene-fm-radio {
  border: 0;
  background: none;
  color: #ffffffbd;
  cursor: pointer;
  font: inherit;
  font-size: 13px;
  text-align: left;
  padding: 0;
  max-width: 100%;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.scene-fm-fee {
  display: block;
  font-size: 12px;
  color: #f4d9aa;
  margin-top: 7px;
}
.scene-fm-categories {
  display: flex;
  gap: 9px;
  overflow-x: auto;
  padding: 8px 0;
  scrollbar-width: thin;
  scrollbar-color: #ffffff60 transparent;
}
.scene-fm-categories button {
  white-space: nowrap;
  border: 1px solid transparent;
  border-radius: 15px;
  padding: 6px 12px;
  background: #ffffff12;
  color: #fffc;
  cursor: pointer;
  font-size: 12px;
}
.scene-fm-categories button.active {
  background: #ffffff2c;
  border-color: #ffffff7a;
  color: var(--on-accent);
}
.scene-fm-actions {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 15px;
  margin: 15px 0;
}
.scene-fm-actions > span {
  font-size: 12px;
}
.scene-fm-auto {
  font-size: 13px;
  display: flex;
  align-items: center;
  gap: 7px;
  margin-top: 24px;
}
.scene-fm-hint {
  font-size: 12px;
  line-height: 1.8;
}
.scene-fm-recovery {
  padding: 12px 0;
  font-size: 13px;
}
.scene-fm-empty {
  padding: 30px 0;
}
.scene-fm-card button:disabled {
  opacity: 0.55;
  cursor: default;
}
.scene-fm-card button:focus-visible {
  outline: 2px solid white;
  outline-offset: 2px;
}
@media (max-width: 850px) {
  .scene-fm-page {
    padding: 20px;
  }
  .scene-fm-card {
    padding: 16px;
  }
  .scene-fm-cover {
    width: 88px;
    height: 88px;
  }
  .scene-fm-content {
    gap: 15px;
    padding: 20px 0;
  }
  .scene-fm-info h2 {
    font-size: 15px;
  }
  .scene-fm-page h1 {
    font-size: 22px;
  }
}
</style>
