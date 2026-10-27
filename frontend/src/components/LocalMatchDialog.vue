<script setup lang="ts">
import { ref, onMounted, onUnmounted } from 'vue';
import type { LocalMatchingModel } from '../composables/useLocalMatching';
import { time } from '../services/player';
const props = defineProps<{ model: LocalMatchingModel }>();
const { target, keyword, results, searching, searchError, busy, error } = props.model;
const dialog = ref<HTMLDialogElement>();
let previous: HTMLElement | null = null;
onMounted(() => {
  previous = document.activeElement as HTMLElement;
  dialog.value?.showModal();
});
onUnmounted(() => previous?.focus());
function close() {
  if (!busy.value) props.model.close();
}
</script>
<template>
  <dialog
    ref="dialog"
    class="local-match-dialog"
    aria-labelledby="local-match-title"
    @cancel.prevent="close"
  >
    <header>
      <h2 id="local-match-title">关联线上歌曲</h2>
      <button class="icon-button" aria-label="关闭歌曲匹配" :disabled="busy" @click="close">
        ×
      </button>
    </header>
    <p class="muted">{{ target?.track.name }} · {{ target?.track.artists.join(' / ') }}</p>
    <p>关联后可查看线上歌曲评论。本地音频和标签保持原样。</p>
    <div v-if="target && model.link(target)" class="notice">
      已关联：{{ model.link(target)?.track?.name || model.link(target)?.songId }}
      <button class="text-button" :disabled="busy" @click="model.clear">解除关联</button>
    </div>
    <form @submit.prevent="model.find(0)">
      <input
        v-model="keyword"
        maxlength="256"
        aria-label="搜索要关联的歌曲"
        placeholder="输入歌曲或歌手"
      /><button class="primary" :disabled="busy || searching || !keyword.trim()">搜索</button>
    </form>
    <p v-if="searching" role="status">正在搜索…</p>
    <p v-if="error || searchError" role="alert">{{ error || searchError }}</p>
    <ul v-if="results" aria-label="歌曲匹配搜索结果">
      <li v-for="song in results.tracks" :key="song.id" :data-song-id="song.id">
        <div>
          <strong>{{ song.name }}</strong
          ><small
            >{{ song.artists.join(' / ') }} · {{ song.album }} ·
            {{ time(song.durationMs / 1000) }}</small
          >
        </div>
        <button
          class="text-button"
          :disabled="busy || searching || model.running.value || song.missing"
          :aria-label="`关联 ${song.name}`"
          @click="model.bind(song.id)"
        >
          关联
        </button>
      </li>
    </ul>
    <p v-if="results && !results.tracks.length && !searching">没有找到对应歌曲，试试其他关键词。</p>
    <footer v-if="results">
      <button
        class="text-button"
        :disabled="busy || searching || !results.offset"
        @click="model.find(Math.max(0, results.offset - 20))"
      >
        上一页</button
      ><span>{{ results.total }} 项结果</span
      ><button
        class="text-button"
        :disabled="busy || searching || !results.more"
        @click="model.find(results.offset + 20)"
      >
        下一页
      </button>
    </footer>
  </dialog>
</template>
