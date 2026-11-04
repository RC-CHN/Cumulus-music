<script setup lang="ts">
import { computed, ref, watch, onMounted, onUnmounted, nextTick } from 'vue';
import type { PlayerModel } from '../composables/usePlayer';
import { time, type PlayerView } from '../services/player';
import Icon from './Icon.vue';
import DesktopLyricToggle from './DesktopLyricToggle.vue';
import LyricFontSize from './LyricFontSize.vue';
import ActionMenu from './ActionMenu.vue';
import { useMotion } from '../composables/useMotion';
import type { CatalogResource } from '../services/catalog';
import {
  playbackMetadata,
  playbackSource,
  type PlaybackSourceDestination,
} from '../services/playbackMetadata';
const props = defineProps<{ player: PlayerModel }>();
const emit = defineEmits<{
  report: [];
  catalog: [target: CatalogResource];
  source: [target: PlaybackSourceDestination];
}>();
const { view, lyrics, lyricLoading, lyricUpdating, lyricError, loaded, busy } = props.player;
const container = ref<HTMLElement>(),
  close = ref<HTMLButtonElement>();
const metadata = computed(() => playbackMetadata(view.value?.track));
const source = computed(() => playbackSource(view.value));
const wordMode = ref(true);
const lyricFontKey = 'cumulus.playback.lyric-font-gain.v1';
const lyricFontGain = ref(0);
try {
  const saved = Number(localStorage.getItem(lyricFontKey));
  if (Number.isInteger(saved) && saved >= -4 && saved <= 4) lyricFontGain.value = saved;
} catch {
  // Session-only sizing remains available when WebView storage is denied.
}
watch(lyricFontGain, (value) => {
  try {
    localStorage.setItem(lyricFontKey, String(value));
  } catch {
    // Keep the active view usable without persistence.
  }
  if (manual.value) void nextTick(selectBrowsedLine);
  else void scroll();
});
const motion = useMotion();
const manual = ref(false);
const resumeButton = ref<HTMLButtonElement>();
const browseButton = ref<HTMLButtonElement>();
const browsedIndex = ref<number>();
const browsingLine = computed(() =>
  browsedIndex.value === undefined ? undefined : lines.value[browsedIndex.value],
);
const browsingTime = computed(() => {
  const seconds = browsingLine.value?.time;
  return seconds === undefined ? undefined : time(seconds);
});
const lyricFollowDelay = 5000;
let lyricFollowTimer: ReturnType<typeof setTimeout> | undefined;
const timedWords = computed(() => wordMode.value && (lyrics.value?.wordLines.length ?? 0) > 0);
type Auxiliary = 'translation' | 'romanization';
const auxiliaries = [
  { key: 'romanization', label: '音', name: '音译歌词' },
  { key: 'translation', label: '译', name: '翻译歌词' },
] as const;
const auxiliaryText = computed(() => {
  const current = lyrics.value;
  function rows(kind: Auxiliary) {
    if (!current) return [];
    if (!timedWords.value) return current[kind].map((row) => row.lyric.trim());
    const wordRows = kind === 'translation' ? current.wordTranslation : current.wordRomanization;
    const byTime = new Map(
      (wordRows.length ? wordRows : current[kind]).map((row) => [row.time, row.lyric.trim()]),
    );
    return current.wordLines.map((line) => byTime.get(line.time));
  }
  return { translation: rows('translation'), romanization: rows('romanization') };
});
const auxiliaryAvailable = computed(() => ({
  translation: auxiliaryText.value.translation.some(Boolean),
  romanization: auxiliaryText.value.romanization.some(Boolean),
}));
const activeAuxiliary = computed<Auxiliary | undefined>(() => {
  const preferences = lyrics.value?.preferences;
  if (preferences?.translationEnable && auxiliaryAvailable.value.translation) return 'translation';
  if (preferences?.romaEnable && auxiliaryAvailable.value.romanization) return 'romanization';
});
const changingLyric = computed(() => lyricLoading.value || lyricUpdating.value);
async function changeLyric(operation: () => Promise<boolean>) {
  if (!lyrics.value || changingLyric.value) return;
  if (await operation()) followLyrics();
}
function toggleAuxiliary(kind: Auxiliary) {
  if (!auxiliaryAvailable.value[kind]) return;
  const next = activeAuxiliary.value === kind ? undefined : kind;
  void changeLyric(() =>
    props.player.preferences({
      translationEnable: next === 'translation',
      romaEnable: next === 'romanization',
    }),
  );
}
type LyricSeekTarget = Pick<PlayerView, 'epoch' | 'playId' | 'queueRevision'>;
const currentEpoch = computed(() => view.value?.epoch);
const currentPlayId = computed(() => view.value?.playId);
const currentQueueRevision = computed(() => view.value?.queueRevision);
const lines = computed(() => {
  const document = lyrics.value;
  if (!document) return [];
  const revision = currentQueueRevision.value;
  const target: LyricSeekTarget | undefined =
    document.epoch === currentEpoch.value && document.playId === currentPlayId.value && revision
      ? { epoch: document.epoch, playId: document.playId, queueRevision: revision }
      : undefined;
  const rows = timedWords.value
    ? document.wordLines.map((l) => ({ time: l.time - document.offset, lyric: l.text }))
    : document.lines;
  return rows.map((row) => ({ ...row, target }));
});
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
function seekable(line: (typeof lines.value)[number]) {
  const time = line.time;
  return (
    !!line.target &&
    !busy.value &&
    loaded.value &&
    time !== undefined &&
    time >= (view.value?.timeline?.start ?? 0) &&
    time <= (view.value?.timeline?.end ?? Infinity)
  );
}
function seekLine(line: (typeof lines.value)[number]) {
  if (line.target && line.time !== undefined && seekable(line))
    void props.player.seekDisplayed(line.target, line.time);
}
// The manual marker selects the row spanning the center of the lyric viewport.
function selectBrowsedLine() {
  const scroller = container.value;
  if (!manual.value || lyricFollowTimer === undefined || !scroller) return;
  const rect = scroller.getBoundingClientRect();
  const middle = rect.top + rect.height / 2;
  let lo = 0;
  let hi = lines.value.length;
  while (lo < hi) {
    const index = Math.floor((lo + hi) / 2);
    const row = scroller.children[index + 1]; // first child is scroll padding
    if (!row) return;
    if (row.getBoundingClientRect().bottom < middle) lo = index + 1;
    else hi = index;
  }
  browsedIndex.value = lines.value[lo]?.time === undefined ? undefined : lo;
}
async function playBrowsedLine() {
  const line = browsingLine.value;
  const target = line?.target;
  if (!line || !target?.playId || line.time === undefined || !seekable(line)) return;
  const result = await props.player.seekDisplayed(target, line.time);
  const current = view.value;
  if (
    !result ||
    current?.epoch !== target.epoch ||
    current.playId !== target.playId ||
    current.queueRevision !== target.queueRevision
  )
    return;
  followLyrics();
  if (current.state === 'paused')
    await props.player.action({
      kind: 'pause_for',
      play_id: target.playId,
      expected_queue_revision: target.queueRevision,
      paused: false,
    });
}
async function scroll() {
  await nextTick();
  if (manual.value) return;
  container.value?.querySelector('.lyric-line.current')?.scrollIntoView({
    block: 'center',
    behavior: motion.reduced.value ? 'instant' : 'smooth',
  });
}
function cancelLyricFollowTimer() {
  if (lyricFollowTimer !== undefined) clearTimeout(lyricFollowTimer);
  lyricFollowTimer = undefined;
}
function clearBrowsedLine() {
  const focused = browseButton.value === document.activeElement;
  browsedIndex.value = undefined;
  if (focused)
    void nextTick(() => {
      if (document.activeElement === document.body) container.value?.focus({ preventScroll: true });
    });
}
function followLyrics() {
  const focused = resumeButton.value === document.activeElement;
  clearBrowsedLine();
  cancelLyricFollowTimer();
  manual.value = false;
  void scroll();
  if (focused)
    void nextTick(() => {
      if (document.activeElement === document.body) container.value?.focus({ preventScroll: true });
    });
}
function pauseLyricFollow(event: Event) {
  if (!lyrics.value?.scrollable || (event instanceof WheelEvent && !event.deltaY)) return;
  manual.value = true;
  cancelLyricFollowTimer();
  lyricFollowTimer = setTimeout(() => {
    lyricFollowTimer = undefined;
    if (view.value?.state === 'playing') followLyrics();
    else clearBrowsedLine();
  }, lyricFollowDelay);
  void nextTick(selectBrowsedLine);
}
watch(motion.reduced, (reduced) => {
  if (reduced && !manual.value) void scroll();
});
watch(active, () => {
  if (!manual.value) void scroll();
});
watch([currentEpoch, currentPlayId], followLyrics);
watch(lines, () => void nextTick(selectBrowsedLine));
watch(container, (scroller, _previous, onCleanup) => {
  if (!scroller) return;
  const observer = new ResizeObserver(selectBrowsedLine);
  observer.observe(scroller);
  onCleanup(() => observer.disconnect());
});
watch(
  () => view.value?.state,
  (state) => {
    if (state === 'playing' && manual.value) followLyrics();
  },
);
let previous: HTMLElement | null = null;
let previousEpoch: string | undefined;
onMounted(() => {
  previous = document.activeElement as HTMLElement;
  previousEpoch = view.value?.epoch;
  close.value?.focus();
  void scroll();
});
onUnmounted(() => {
  cancelLyricFollowTimer();
  if (view.value?.epoch === previousEpoch && previous?.isConnected)
    previous.focus({ preventScroll: true });
});
function wordProgress(word: { time: number; duration: number }) {
  const seconds = (view.value?.position ?? 0) + (lyrics.value?.offset ?? 0);
  return `${Math.min(100, Math.max(0, word.duration ? ((seconds - word.time) / word.duration) * 100 : seconds >= word.time ? 100 : 0))}%`;
}
const displayedAuxiliary = computed(() =>
  activeAuxiliary.value ? auxiliaryText.value[activeAuxiliary.value] : [],
);
</script>
<template>
  <section
    class="lyrics-panel"
    :style="{ '--lyric-font-gain': `${lyricFontGain}px` }"
    aria-label="播放详情与歌词"
    @keydown.esc.stop="player.lyricsOpen.value = false"
  >
    <header>
      <span>{{ view?.playId ? '正在播放' : '播放详情' }}</span
      ><DesktopLyricToggle v-if="view" :epoch="view.epoch" /><button
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
        <h1 :title="view?.track?.name">{{ view?.track?.name }}</h1>
        <p v-if="view?.track" class="vinyl-credits" aria-label="歌手">
          <template v-for="(artist, index) in metadata.artists" :key="index">
            <span v-if="index > 0" aria-hidden="true"> / </span>
            <button
              v-if="artist.target"
              type="button"
              class="text-button vinyl-credit"
              :aria-label="`查看歌手：${artist.name}`"
              :title="artist.name"
              @click="emit('catalog', artist.target)"
            >
              {{ artist.name }}
            </button>
            <span v-else class="vinyl-credit" :title="artist.name">{{ artist.name }}</span>
          </template>
        </p>
        <small v-if="view?.track" class="vinyl-album">
          <button
            v-if="metadata.album.target"
            type="button"
            class="text-button vinyl-credit"
            :aria-label="`查看专辑：${metadata.album.name}`"
            :title="metadata.album.name"
            @click="emit('catalog', metadata.album.target)"
          >
            {{ metadata.album.name }}
          </button>
          <span v-else class="vinyl-credit" :title="metadata.album.name">{{
            metadata.album.name
          }}</span>
        </small>
        <small v-if="source" class="vinyl-source">
          <span>来源：</span>
          <button
            v-if="source.target"
            type="button"
            class="text-button vinyl-credit"
            :aria-label="`查看播放来源：${source.name}`"
            :title="source.name"
            @click="emit('source', source.target)"
          >
            {{ source.name }}
          </button>
          <span v-else>{{ source.name }}</span>
        </small>
        <span v-if="view?.timeline?.mode === 'trial'" class="trial-badge">当前为试听片段</span>
      </div>
      <div class="lyrics-column">
        <div v-if="lyricLoading" class="empty-state" role="status">
          <span class="spinner" />正在加载歌词…
        </div>
        <div v-else-if="lyricError" class="empty-state">
          {{ lyricError }}<button class="text-button" @click="player.loadLyrics">重新加载</button>
        </div>
        <template v-else-if="lyrics">
          <div
            v-if="lyrics.fromSidecar || lyrics.fromEmbedded || lyrics.networkFailed"
            class="lyric-cache-note"
          >
            {{
              lyrics.fromSidecar || lyrics.fromEmbedded
                ? '正在显示本地歌词'
                : lyrics.fromCache
                  ? '网络暂不可用，正在显示缓存歌词'
                  : '暂未获取到歌词'
            }}<button v-if="lyrics.networkFailed" class="text-button" @click="player.loadLyrics">
              重试
            </button>
          </div>
          <div v-if="!lines.length" class="empty-state">
            {{ lyrics.displayType === 'pure' ? '纯音乐，请欣赏' : '暂无歌词' }}
          </div>
          <div v-else class="lyric-scroll-shell" :class="{ browsing: manual }">
            <div
              ref="container"
              class="lyric-scroll"
              tabindex="-1"
              @wheel.passive="pauseLyricFollow"
              @touchmove.passive="pauseLyricFollow"
              @scroll.passive="selectBrowsedLine"
            >
              <div class="lyric-padding" />
              <button
                v-for="(line, index) in lines"
                :key="`${line.target?.epoch}:${line.target?.playId}:${index}`"
                class="lyric-line"
                :class="{ current: index === active, browsed: index === browsedIndex }"
                :data-line="index"
                :disabled="!seekable(line)"
                @click="seekLine(line)"
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
                <small v-if="displayedAuxiliary[index]">{{ displayedAuxiliary[index] }}</small>
              </button>
              <div class="lyric-padding" />
            </div>
            <button
              v-if="browsingLine && browsingTime"
              ref="browseButton"
              type="button"
              class="lyric-browse-time"
              :aria-label="`从歌词时间 ${browsingTime} 播放`"
              :disabled="!seekable(browsingLine)"
              @click="playBrowsedLine"
            >
              <Icon name="play" />{{ browsingTime }}
            </button>
          </div>
        </template>
        <div v-else-if="!view?.playId" class="empty-state">暂无播放中的歌曲</div>
        <div class="lyric-tools">
          <template v-if="lyrics">
            <LyricFontSize v-if="lines.length" v-model="lyricFontGain" />
            <template v-if="lyrics.scrollable">
              <button
                class="icon-button"
                aria-label="歌词延后半秒"
                :aria-disabled="changingLyric"
                @click="changeLyric(() => player.offset(-1))"
              >
                −
              </button>
              <small>{{ lyrics.offset.toFixed(1) }}s</small>
              <button
                class="icon-button"
                aria-label="歌词提前半秒"
                :aria-disabled="changingLyric"
                @click="changeLyric(() => player.offset(1))"
              >
                +
              </button>
            </template>
            <template v-for="auxiliary in auxiliaries" :key="auxiliary.key">
              <button
                v-if="auxiliaryAvailable[auxiliary.key]"
                class="text-button"
                :aria-label="auxiliary.name"
                :title="auxiliary.name"
                :aria-pressed="activeAuxiliary === auxiliary.key"
                :aria-disabled="changingLyric"
                @click="toggleAuxiliary(auxiliary.key)"
              >
                {{ auxiliary.label }}
              </button>
            </template>
            <button
              v-if="lyrics.wordLines.length"
              class="text-button"
              :aria-pressed="wordMode"
              @click="wordMode = !wordMode"
            >
              逐字
            </button>
            <button v-if="manual" ref="resumeButton" class="text-button" @click="followLyrics">
              回到当前
            </button>
          </template>
          <ActionMenu
            v-if="!view?.track?.voiceId"
            label="歌词操作"
            :items="[{ key: 'report', label: '歌词报错', icon: 'edit' }]"
            @select="emit('report')"
          />
        </div>
      </div>
    </div>
  </section>
</template>
