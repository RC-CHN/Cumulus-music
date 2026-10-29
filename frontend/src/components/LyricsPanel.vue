<script setup lang="ts">
import { computed, ref, watch, onMounted, onUnmounted, nextTick } from 'vue';
import type { PlayerModel } from '../composables/usePlayer';
import Icon from './Icon.vue';
import DesktopLyricToggle from './DesktopLyricToggle.vue';
import { useMotion } from '../composables/useMotion';
const props = defineProps<{ player: PlayerModel }>();
const { view, lyrics, lyricLoading, lyricError, loaded } = props.player;
const container = ref<HTMLElement>(),
  close = ref<HTMLButtonElement>();
const wordMode = ref(true);
const motion = useMotion();
const manual = ref(false);
const timedWords = computed(() => wordMode.value && (lyrics.value?.wordLines.length ?? 0) > 0);
const lines = computed(() =>
  timedWords.value
    ? lyrics.value!.wordLines.map((l) => ({
        time: l.time - (lyrics.value?.offset ?? 0),
        lyric: l.text,
      }))
    : (lyrics.value?.lines ?? []),
);
const active = computed(() => {
  let found = -1,
    latest = -Infinity;
  lines.value.forEach((l, i) => {
    if (l.time !== undefined && l.time <= (view.value?.position ?? 0) && l.time >= latest) {
      found = i;
      latest = l.time;
    }
  });
  return found;
});
function seekable(time: number | undefined) {
  return (
    loaded.value &&
    time !== undefined &&
    time >= (view.value?.timeline?.start ?? 0) &&
    time <= (view.value?.timeline?.end ?? Infinity)
  );
}
async function scroll() {
  await nextTick();
  container.value?.querySelector('.lyric-line.current')?.scrollIntoView({
    block: 'center',
    behavior: motion.reduced.value ? 'instant' : 'smooth',
  });
}
watch(motion.reduced, (reduced) => {
  if (reduced && !manual.value) void scroll();
});
watch(active, () => {
  if (!manual.value) void scroll();
});
watch(
  () => view.value?.playId,
  () => (manual.value = false),
);
let previous: HTMLElement | null = null;
onMounted(() => {
  previous = document.activeElement as HTMLElement;
  close.value?.focus();
  void scroll();
});
onUnmounted(() => previous?.focus());
function wordProgress(word: { time: number; duration: number }) {
  const seconds = (view.value?.position ?? 0) + (lyrics.value?.offset ?? 0);
  return `${Math.min(100, Math.max(0, word.duration ? ((seconds - word.time) / word.duration) * 100 : seconds >= word.time ? 100 : 0))}%`;
}
function aligned(rows: { time?: number; lyric: string }[] | undefined, index: number) {
  if (!timedWords.value) return rows?.[index]?.lyric;
  const current = lyrics.value;
  if (!current) return;
  if (rows === current.translation && current.wordTranslation.length)
    rows = current.wordTranslation;
  if (rows === current.romanization && current.wordRomanization.length)
    rows = current.wordRomanization;
  const time = current.wordLines[index]?.time;
  return rows?.find((l) => l.time === time)?.lyric;
}
</script>
<template>
  <section
    class="lyrics-panel"
    aria-label="播放详情与歌词"
    @keydown.esc.stop="player.lyricsOpen.value = false"
  >
    <header>
      <span>正在播放</span><DesktopLyricToggle v-if="view" :epoch="view.epoch" /><button
        ref="close"
        class="icon-button"
        aria-label="收起播放详情"
        @click="player.lyricsOpen.value = false"
      >
        <Icon name="down" />
      </button>
    </header>
    <div class="lyrics-body">
      <div class="vinyl-info">
        <div class="vinyl-record">
          <img
            v-if="view?.track?.coverUrl"
            :src="view.track.coverUrl"
            alt="专辑封面"
            referrerpolicy="no-referrer"
          /><Icon v-else name="music" />
        </div>
        <h1>{{ view?.track?.name }}</h1>
        <p>{{ view?.track?.artists.join(' / ') }}</p>
        <small>{{ view?.track?.album }}</small
        ><span v-if="view?.timeline?.mode === 'trial'" class="trial-badge">当前为试听片段</span>
      </div>
      <div class="lyrics-column">
        <div v-if="lyricLoading" class="empty-state" role="status">
          <span class="spinner" />正在加载歌词…
        </div>
        <div v-else-if="lyricError" class="empty-state">
          {{ lyricError }}<button class="text-button" @click="player.loadLyrics">重新加载</button>
        </div>
        <template v-else-if="lyrics">
          <div v-if="lyrics.networkFailed" class="lyric-cache-note">
            {{ lyrics.fromCache ? '网络暂不可用，正在显示缓存歌词' : '暂未获取到歌词'
            }}<button class="text-button" @click="player.loadLyrics">重试</button>
          </div>
          <div v-if="!lines.length" class="empty-state">
            {{ lyrics.displayType === 'pure' ? '纯音乐，请欣赏' : '暂无歌词' }}
          </div>
          <div
            v-else
            ref="container"
            class="lyric-scroll"
            @wheel.passive="manual = true"
            @touchmove.passive="manual = true"
          >
            <div class="lyric-padding" />
            <button
              v-for="(line, index) in lines"
              :key="index"
              class="lyric-line"
              :class="{ current: index === active }"
              :data-line="index"
              :disabled="!seekable(line.time)"
              @click="
                line.time !== undefined && player.action({ kind: 'seek', seconds: line.time })
              "
            >
              <span v-if="timedWords && lyrics.wordLines[index]?.words.length" class="word-line"
                ><span
                  v-for="(word, wi) in lyrics.wordLines[index]?.words"
                  :key="wi"
                  class="timed-word"
                  :style="{ '--word-progress': wordProgress(word) }"
                  >{{ word.text }}</span
                ></span
              >
              <span v-else>{{ line.lyric || '♪' }}</span>
              <small
                v-if="lyrics.preferences.translationEnable && aligned(lyrics.translation, index)"
                >{{ aligned(lyrics.translation, index) }}</small
              ><small v-if="lyrics.preferences.romaEnable && aligned(lyrics.romanization, index)">{{
                aligned(lyrics.romanization, index)
              }}</small>
            </button>
            <div class="lyric-padding" />
          </div>
          <div class="lyric-tools">
            <button
              class="text-button"
              :aria-pressed="lyrics.preferences.translationEnable"
              @click="
                player.preferences({
                  ...lyrics.preferences,
                  translationEnable: !lyrics.preferences.translationEnable,
                })
              "
            >
              译</button
            ><button
              class="text-button"
              :aria-pressed="lyrics.preferences.romaEnable"
              @click="
                player.preferences({
                  ...lyrics.preferences,
                  romaEnable: !lyrics.preferences.romaEnable,
                })
              "
            >
              音</button
            ><button
              v-if="lyrics.wordLines.length"
              class="text-button"
              :aria-pressed="wordMode"
              @click="wordMode = !wordMode"
            >
              逐字</button
            ><button
              class="icon-button"
              aria-label="歌词延后半秒"
              :disabled="!lyrics.scrollable"
              @click="player.offset(-1)"
            >
              −</button
            ><small>{{ lyrics.offset.toFixed(1) }}s</small
            ><button
              class="icon-button"
              aria-label="歌词提前半秒"
              :disabled="!lyrics.scrollable"
              @click="player.offset(1)"
            >
              +</button
            ><button
              v-if="manual"
              class="text-button"
              @click="
                manual = false;
                scroll();
              "
            >
              回到当前
            </button>
          </div>
        </template>
      </div>
    </div>
  </section>
</template>
