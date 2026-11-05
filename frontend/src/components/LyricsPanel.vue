<script setup lang="ts">
import { computed, ref, watch, onMounted, onUnmounted, nextTick } from 'vue';
import type { PlayerModel } from '../composables/usePlayer';
import { time, type PlayerView } from '../services/player';
import Icon from './Icon.vue';
import DesktopLyricToggle from './DesktopLyricToggle.vue';
import LyricFontSize from './LyricFontSize.vue';
import ActionMenu from './ActionMenu.vue';
import ModalTransition from './ModalTransition.vue';
import VinylModeSheet from './VinylModeSheet.vue';
import VinylCommentsPane from './VinylCommentsPane.vue';
import type { VinylCommentsModel } from '../composables/useVinylComments';
import { vinylCoverLyric, type VinylDisplayMode } from '../services/vinylDisplay';
import { useMotion } from '../composables/useMotion';
import type { CatalogResource } from '../services/catalog';
import {
  playbackMetadata,
  playbackSource,
  type PlaybackSourceDestination,
} from '../services/playbackMetadata';
const props = defineProps<{
  player: PlayerModel;
  commentsAvailable: boolean;
  commentModel: VinylCommentsModel;
  commentCount?: number;
}>();
const commentsOpen = defineModel<boolean>('commentsOpen', { required: true });
const preferredMode = defineModel<VinylDisplayMode>('mode', { required: true });
const emit = defineEmits<{
  report: [];
  catalog: [target: CatalogResource];
  source: [target: PlaybackSourceDestination];
  fullComments: [hot: boolean];
}>();
const { view, lyrics, lyricLoading, lyricUpdating, lyricError, loaded, busy } = props.player;
const container = ref<HTMLElement>(),
  close = ref<HTMLButtonElement>(),
  modeEntry = ref<HTMLButtonElement>();
const modeSheetOpen = ref(false);
const modeAvailable = computed(
  () => !!view.value?.track && view.value.resourceKind !== 'voice' && !view.value.track.voiceId,
);
const displayMode = computed(() => (modeAvailable.value ? preferredMode.value : 'classic'));
watch(modeAvailable, (available) => {
  if (!available) modeSheetOpen.value = false;
});
function closeModeSheet() {
  modeSheetOpen.value = false;
}
function modeEntryFocus() {
  return modeEntry.value ?? null;
}
function panelCloseFocus() {
  return close.value ?? null;
}
function escapePanel() {
  // Native modal cancel closes only the mode sheet.
  if (!modeSheetOpen.value) props.player.lyricsOpen.value = false;
}
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
const commentsVisible = computed(() => props.commentsAvailable && commentsOpen.value);
const commentPane = ref<InstanceType<typeof VinylCommentsPane>>();
const commentEntry = ref<HTMLButtonElement>();
const wheelNudge = ref(0);
const wheelSliding = ref(false);
let accumulated = 0;
let idleWheel: ReturnType<typeof setTimeout> | undefined;
let unlockWheel: ReturnType<typeof setTimeout> | undefined;
let returnCommentFocus: HTMLElement | undefined;
function resetWheel() {
  clearTimeout(idleWheel);
  accumulated = 0;
  wheelNudge.value = 0;
}
function vinylWheel(event: WheelEvent) {
  if (!props.commentsAvailable || wheelSliding.value || !event.deltaY) return;
  const next = accumulated + event.deltaY;
  accumulated = commentsVisible.value ? Math.min(0, next) : Math.max(0, next);
  clearTimeout(idleWheel);
  idleWheel = setTimeout(resetWheel, 250);
  if (Math.abs(accumulated) >= 480) {
    commentsOpen.value = !commentsVisible.value;
  } else if (!motion.reduced.value) {
    wheelNudge.value = -accumulated / 2;
  }
}
watch(commentsVisible, async (active) => {
  resetWheel();
  clearTimeout(unlockWheel);
  wheelSliding.value = !motion.reduced.value;
  if (wheelSliding.value) unlockWheel = setTimeout(() => (wheelSliding.value = false), 500);
  if (active) {
    const focused = document.activeElement;
    returnCommentFocus =
      focused instanceof HTMLElement && focused !== document.body ? focused : undefined;
    await nextTick();
    if (commentsVisible.value) commentPane.value?.focusBack();
  } else {
    await nextTick();
    if (commentsVisible.value || !props.player.lyricsOpen.value) return;
    const anchor = returnCommentFocus;
    returnCommentFocus = undefined;
    (anchor?.isConnected && !anchor.closest('[inert]')
      ? anchor
      : (commentEntry.value ?? close.value)
    )?.focus({ preventScroll: true });
    void scroll();
  }
});
watch(motion.reduced, (reduced) => {
  if (reduced) {
    clearTimeout(unlockWheel);
    wheelSliding.value = false;
    resetWheel();
  }
});
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
const currentLyric = computed(() => {
  const document = lyrics.value;
  return document?.epoch === currentEpoch.value && document?.playId === currentPlayId.value
    ? document
    : undefined;
});
const coverPosition = ref<number>();
watch(
  [displayMode, currentLyric],
  ([mode, document], _previous, onCleanup) => {
    coverPosition.value = undefined;
    if (mode !== 'cover' || !document?.playId) return;
    let tick: ReturnType<typeof setTimeout> | undefined;
    const stop = watch(
      () => view.value?.position,
      () => {
        tick ??= setTimeout(() => {
          coverPosition.value = view.value?.position ?? undefined;
          tick = undefined;
        }, 500);
      },
      { immediate: true },
    );
    onCleanup(() => {
      stop();
      clearTimeout(tick);
    });
  },
  { immediate: true },
);
const coverLine = computed(() =>
  coverPosition.value === undefined
    ? ''
    : vinylCoverLyric(currentLyric.value?.lines ?? [], coverPosition.value),
);
const lines = computed(() => {
  const document = currentLyric.value;
  if (!document) return [];
  const revision = currentQueueRevision.value;
  const target: LyricSeekTarget | undefined = revision
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
  if (manual.value || commentsVisible.value) return;
  const scroller = container.value;
  const current = scroller?.querySelector('.lyric-line.current');
  if (!scroller || !current) return;
  const behavior = motion.reduced.value ? 'instant' : 'smooth';
  if (!props.commentsAvailable) {
    current.scrollIntoView({ block: 'center', behavior });
    return;
  }
  // The experimental stack clips a neighboring full pane. Keep lyric
  // centering scoped to its scroller so it cannot move the outer viewport.
  const box = scroller.getBoundingClientRect();
  const line = current.getBoundingClientRect();
  scroller.scrollTo({
    top: scroller.scrollTop + line.top - box.top + (line.height - box.height) / 2,
    behavior,
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
watch([currentEpoch, currentPlayId, displayMode], followLyrics);
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
  if (commentsVisible.value) commentPane.value?.focusBack();
  void scroll();
});
onUnmounted(() => {
  resetWheel();
  clearTimeout(unlockWheel);
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
    :data-vinyl-mode="displayMode"
    :data-vinyl-tab="commentsVisible ? 'comments' : 'lyrics'"
    :style="{ '--lyric-font-gain': `${lyricFontGain}px` }"
    aria-label="播放详情与歌词"
    @keydown.esc.stop="escapePanel"
  >
    <header>
      <span>{{ view?.playId ? '正在播放' : '播放详情' }}</span>
      <DesktopLyricToggle v-if="view" :epoch="view.epoch" />
      <div class="vinyl-header-actions">
        <button
          v-if="modeAvailable && !commentsVisible"
          ref="modeEntry"
          class="secondary vinyl-mode-entry"
          type="button"
          aria-haspopup="dialog"
          :aria-expanded="modeSheetOpen"
          @click="modeSheetOpen = true"
        >
          播放器模式
        </button>
        <button
          ref="close"
          class="icon-button"
          aria-label="收起播放详情"
          @click="player.lyricsOpen.value = false"
        >
          <Icon name="down" />
        </button>
      </div>
    </header>
    <div class="vinyl-tab-viewport" @wheel.passive="vinylWheel">
      <div
        class="vinyl-tab-stack"
        :class="{ dragging: wheelNudge !== 0 }"
        :style="{ '--vinyl-wheel-nudge': `${wheelNudge}px` }"
      >
        <div class="lyrics-body" :inert="commentsVisible">
          <button
            v-if="commentsAvailable"
            ref="commentEntry"
            type="button"
            class="text-button vinyl-scroll-comment"
            @click="commentsOpen = true"
          >
            <Icon name="down" />滚动查看评论<Icon name="down" />
          </button>
          <div class="vinyl-info">
            <div v-if="displayMode !== 'lyric'" class="vinyl-record">
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
            <small v-if="displayMode === 'classic' && view?.track" class="vinyl-album">
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
            <small v-if="displayMode === 'classic' && source" class="vinyl-source">
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
            <p v-if="displayMode === 'cover'" class="vinyl-cover-line">{{ coverLine }}</p>
          </div>
          <div v-if="displayMode !== 'cover'" class="lyrics-column">
            <div v-if="lyricLoading" class="empty-state" role="status">
              <span class="spinner" />正在加载歌词…
            </div>
            <div v-else-if="lyricError" class="empty-state">
              {{ lyricError
              }}<button class="text-button" @click="player.loadLyrics">重新加载</button>
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
                }}<button
                  v-if="lyrics.networkFailed"
                  class="text-button"
                  @click="player.loadLyrics"
                >
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
                  @wheel.stop.passive="pauseLyricFollow"
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
                    <span
                      v-if="timedWords && lyrics.wordLines[index]?.words.length"
                      class="word-line"
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
            <div v-if="displayMode === 'classic'" class="lyric-tools">
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
        <VinylCommentsPane
          v-if="commentsAvailable && view?.track"
          ref="commentPane"
          :track="view.track"
          :active="commentsVisible"
          :count="commentCount"
          :model="commentModel"
          @back="commentsOpen = false"
          @full="emit('fullComments', false)"
          @hot="emit('fullComments', true)"
        />
      </div>
    </div>
    <ModalTransition :return-focus="modeEntryFocus" :fallback-focus="panelCloseFocus">
      <VinylModeSheet
        v-if="modeSheetOpen && modeAvailable"
        :mode="preferredMode"
        @select="preferredMode = $event"
        @close="closeModeSheet"
      />
    </ModalTransition>
  </section>
</template>
