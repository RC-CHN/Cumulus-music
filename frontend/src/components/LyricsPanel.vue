<script setup lang="ts">
import { computed, ref, shallowRef, watch, onMounted, onUnmounted, nextTick } from 'vue';
import type { PlayerModel } from '../composables/usePlayer';
import { time, type PlayerView } from '../services/player';
import { lyricContributorRows, type LyricContributorTarget } from '../services/lyricContributors';
import Icon from './Icon.vue';
import { useCoverImage } from '../composables/useCoverImage';
import { usePodcastFollow } from '../composables/usePodcastFollow';
import { useCore } from '../services/core';
import type { AccountView } from '../services/contracts';
import type { PodcastSearchFollow } from '../services/podcast';
import {
  playbackVoiceDetail,
  voiceCreatorTarget,
  voicePodcastTarget,
  voicePodcastTargetIsCurrent,
  type VoiceCreatorTarget,
  type VoicePodcastTarget,
} from '../services/voiceDetail';
import DesktopLyricToggle from './DesktopLyricToggle.vue';
import LyricFontSize from './LyricFontSize.vue';
import ActionMenu from './ActionMenu.vue';
import ModalTransition from './ModalTransition.vue';
import VinylModeSheet from './VinylModeSheet.vue';
import VinylCommentsPane from './VinylCommentsPane.vue';
import VinylSimilarPane from './VinylSimilarPane.vue';
import VinylWikiPane from './VinylWikiPane.vue';
import VoiceDescriptionCard from './VoiceDescriptionCard.vue';
import VoicePodcastCard from './VoicePodcastCard.vue';
import VoicePodcastUncollectDialog from './VoicePodcastUncollectDialog.vue';
import VoiceRecommendationsCard from './VoiceRecommendationsCard.vue';
import type { VoiceRecommendationsModel } from '../composables/useVoiceRecommendations';
import type {
  VoiceRecommendationsPage,
  VoiceRecommendationTarget,
} from '../services/voiceRecommendations';
import VinylCommentDialog from './VinylCommentDialog.vue';
import type {
  VinylCommentComposer,
  VinylComposerOpening,
  VinylComposerTarget,
} from '../composables/useVinylCommentComposer';
import type { VinylCommentsModel } from '../composables/useVinylComments';
import type { VinylSimilarModel } from '../composables/useVinylSimilar';
import type { VinylWikiModel } from '../composables/useVinylWiki';
import {
  vinylDetailTabs,
  type VinylDetailTab,
  type VinylRecommendationPlacement,
  type VinylSimilarLane,
} from '../services/vinylSimilar';
import { vinylCoverLyric, type VinylDisplayMode } from '../services/vinylDisplay';
import { vinylCopyText, vinylSelection } from '../services/vinylTextCopy';
import { useMotion } from '../composables/useMotion';
import type { CatalogResource } from '../services/catalog';
import {
  playbackMetadata,
  playbackSource,
  type PlaybackSourceDestination,
} from '../services/playbackMetadata';
const props = defineProps<{
  player: PlayerModel;
  account: AccountView;
  wikiAvailable: boolean;
  recommendationPlacement: VinylRecommendationPlacement;
  activeDetailTab: VinylDetailTab;
  similarModel: VinylSimilarModel;
  wikiModel: VinylWikiModel;
  voiceRecommendations: VoiceRecommendationsModel;
  commentsAvailable: boolean;
  commentModel: VinylCommentsModel;
  commentCount?: number;
  commentWriteTarget?: VinylComposerTarget;
  commentComposer: VinylCommentComposer;
}>();
const commentsOpen = defineModel<boolean>('commentsOpen', { required: true });
const detailTab = defineModel<VinylDetailTab>('detailTab', { required: true });
const preferredMode = defineModel<VinylDisplayMode>('mode', { required: true });
const emit = defineEmits<{
  report: [];
  commentReportExpired: [];
  catalog: [target: CatalogResource];
  source: [target: PlaybackSourceDestination];
  voiceCreator: [target: VoiceCreatorTarget];
  voicePodcast: [target: VoicePodcastTarget];
  voiceRecommendation: [target: VoiceRecommendationTarget];
  voicePodcastLogin: [];
  lyricContributor: [target: LyricContributorTarget];
  fullComments: [hot: boolean];
  relatedPlaylist: [id: string];
  retryRecommendation: [lane: VinylSimilarLane];
}>();
function independentComment(opening: VinylComposerOpening) {
  if (!props.commentComposer.live(opening)) return;
  props.commentComposer.close(opening);
  emit('fullComments', false);
}
function commentOpener() {
  return props.commentComposer.current.value?.opener ?? null;
}
const { view, lyrics, lyricLoading, lyricUpdating, lyricError, loaded, busy } = props.player;
const panel = ref<HTMLElement>(),
  container = ref<HTMLElement>(),
  close = ref<HTMLButtonElement>(),
  modeEntry = ref<HTMLButtonElement>();
const voiceCards = ref<HTMLElement>();
const modeSheetOpen = ref(false);
const uncollect = shallowRef<{
  target: VoicePodcastTarget;
  expected: PodcastSearchFollow;
}>();
let uncollectOpener: HTMLElement | null = null;
const modeAvailable = computed(
  () => !!view.value?.track && view.value.resourceKind !== 'voice' && !view.value.track.voiceId,
);
const displayMode = computed(() => (modeAvailable.value ? preferredMode.value : 'classic'));
const classicTitleAlias = computed(() =>
  displayMode.value === 'classic' ? view.value?.titleAlias : undefined,
);
const displayedTitle = computed(
  () => view.value?.track?.name || (classicTitleAlias.value ? '未知歌曲' : ''),
);
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
  if (!modeSheetOpen.value && !uncollect.value) props.player.lyricsOpen.value = false;
}
const metadata = computed(() => playbackMetadata(view.value?.track));
const source = computed(() => playbackSource(view.value));
const voiceMetadata = computed(() => playbackVoiceDetail(view.value));
const recommendedPodcasts = computed(() => {
  const page = props.voiceRecommendations.view.value;
  return page?.cards.length ? page : undefined;
});
function openVoiceRecommendation(page: VoiceRecommendationsPage, radioId: string) {
  const target = props.voiceRecommendations.target(page, radioId);
  if (target) emit('voiceRecommendation', target);
}
const creatorTarget = computed(() => voiceCreatorTarget(view.value));
const cover = useCoverImage(() => ({
  owner: view.value?.epoch ?? '',
  source: view.value?.track?.coverUrl,
}));
function openVoiceCreator() {
  const target = voiceCreatorTarget(view.value);
  if (target) emit('voiceCreator', target);
}
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
const podcastTarget = computed(() => voicePodcastTarget(view.value));
const podcastFollow = usePodcastFollow(
  useCore(),
  () => props.account,
  () =>
    props.player.lyricsOpen.value &&
    !commentsVisible.value &&
    podcastTarget.value?.epoch === props.account.epoch &&
    !voiceMetadata.value?.radio?.ownedByViewer,
  () => emit('commentReportExpired'),
  {
    scope: () => podcastTarget.value?.playId ?? '',
    command: (id, expected) => {
      const target = podcastTarget.value;
      if (target?.radioId === id)
        return { kind: 'change_vinyl_subscription', play_id: target.playId, expected };
    },
  },
);
function ownsPodcastCard(target: VoicePodcastTarget) {
  return (
    props.player.lyricsOpen.value &&
    !commentsVisible.value &&
    target.epoch === props.account.epoch &&
    voiceMetadata.value?.radio?.ownedByViewer !== true &&
    voicePodcastTargetIsCurrent(target, view.value)
  );
}
function activateVoicePodcast(
  target: VoicePodcastTarget,
  expected: PodcastSearchFollow | undefined,
  opener: HTMLElement,
) {
  if (!ownsPodcastCard(target)) return;
  if (props.account.status !== 'authenticated') {
    emit('voicePodcastLogin');
    return;
  }
  if (expected?.kind === 'toggle' && expected.subscribed && !expected.busy) {
    uncollectOpener = opener;
    uncollect.value = { target, expected };
    return;
  }
  if (expected) void podcastFollow.activate(target.radioId, expected);
  else if (!podcastFollow.refreshing.value) void podcastFollow.retry();
}
function confirmVoicePodcastUncollect() {
  const saved = uncollect.value;
  uncollect.value = undefined;
  if (saved && props.account.status === 'authenticated' && ownsPodcastCard(saved.target))
    void podcastFollow.activate(saved.target.radioId, saved.expected);
}
watch(
  () => {
    const current = uncollect.value;
    return (
      !current || (props.account.status === 'authenticated' && ownsPodcastCard(current.target))
    );
  },
  (valid) => {
    if (!valid) uncollect.value = undefined;
  },
  { flush: 'sync' },
);
const tabs = computed(() => vinylDetailTabs(props.wikiAvailable, props.recommendationPlacement));
const tabLabel: Record<VinylDetailTab, string> = {
  lyrics: '歌词',
  wiki: '百科',
  similar: '相似推荐',
};
const tabList = ref<HTMLElement>();
const wikiScroll = ref<HTMLElement>();
const recommendationProps = computed(() => ({
  playId: view.value?.playId ?? '',
  songs: props.similarModel.songs.value,
  playlists: props.similarModel.playlists.value,
  songsState: props.similarModel.songsState.value,
  playlistsState: props.similarModel.playlistsState.value,
  notice: props.similarModel.notice.value,
  pending: props.similarModel.pending.value,
  expanded: props.similarModel.expanded.value,
  canPlay: props.similarModel.canPlay.value && loaded.value && !busy.value,
}));
function focusDetailTab(tab: VinylDetailTab) {
  const target = tabList.value?.querySelector<HTMLElement>('[data-vinyl-detail-tab="' + tab + '"]');
  (target ?? close.value)?.focus({ preventScroll: true });
}
function detailKey(event: KeyboardEvent) {
  const keys = tabs.value;
  const index = keys.indexOf(props.activeDetailTab);
  let next = -1;
  if (event.key === 'Home') next = 0;
  else if (event.key === 'End') next = keys.length - 1;
  else if (event.key === 'ArrowLeft') next = (index + keys.length - 1) % keys.length;
  else if (event.key === 'ArrowRight') next = (index + 1) % keys.length;
  if (next < 0) return;
  event.preventDefault();
  detailTab.value = keys[next];
  void nextTick(() => focusDetailTab(keys[next]));
}
watch(
  [
    tabs,
    () => props.activeDetailTab,
    () => props.recommendationPlacement,
    () => view.value?.playId,
  ],
  async ([next, tab]) => {
    const focused = document.activeElement;
    const panel = close.value?.closest('.lyrics-panel');
    if (!(focused instanceof HTMLElement) || !panel?.contains(focused)) return;
    const button = focused.dataset.vinylDetailTab as VinylDetailTab | undefined;
    const body = focused.closest<HTMLElement>('.vinyl-detail-content');
    if (!button && !body) return;
    await nextTick();
    if (!props.player.lyricsOpen.value) return;
    const active = document.activeElement;
    if (active !== focused && active !== document.body) return;
    if (
      !focused.isConnected ||
      (button && !next.includes(button)) ||
      (body && body.id !== 'vinyl-' + tab + '-content')
    )
      focusDetailTab(tab);
  },
);
watch(
  () => props.activeDetailTab,
  async (active) => {
    if (active === 'lyrics') {
      await nextTick();
      if (!manual.value) void scroll();
    }
  },
);
watch(
  () => view.value?.playId,
  () => {
    wikiScroll.value?.scrollTo({ top: 0, behavior: motion.reduced.value ? 'instant' : 'smooth' });
  },
);
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
watch(
  [currentEpoch, currentPlayId],
  () => {
    const cards = voiceCards.value;
    if (cards?.scrollTop)
      cards.scrollTo({ top: 0, behavior: motion.reduced.value ? 'auto' : 'smooth' });
  },
  { flush: 'post' },
);
const currentLyric = computed(() => {
  const document = lyrics.value;
  return document?.epoch === currentEpoch.value && document?.playId === currentPlayId.value
    ? document
    : undefined;
});
const contributors = computed(() => lyricContributorRows(currentLyric.value, view.value));
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
function clickLyricLine(event: MouseEvent, line: (typeof lines.value)[number]) {
  if (event.detail && container.value && vinylSelection(container.value, window.getSelection()))
    return;
  seekLine(line);
}
function lyricLineKey(event: KeyboardEvent, line: (typeof lines.value)[number]) {
  if (event.key !== 'Enter' && event.key !== ' ') return;
  event.preventDefault();
  event.stopPropagation();
  if (!event.repeat && !event.isComposing) seekLine(line);
}
let selectingPointer: number | undefined;
function beginLyricSelection(event: PointerEvent) {
  if (
    event.isPrimary &&
    event.button === 0 &&
    event.pointerType === 'mouse' &&
    event.target instanceof Element &&
    event.target.closest('[data-lyric-text]')
  ) {
    selectingPointer = event.pointerId;
  }
}
function endLyricSelection(event: PointerEvent) {
  if (event.pointerId === selectingPointer) selectingPointer = undefined;
}
function resetLyricSelectionPointer() {
  selectingPointer = undefined;
}
function copyVinylSelection(event: ClipboardEvent) {
  const root = panel.value;
  if (
    event.defaultPrevented ||
    !event.clipboardData ||
    !root ||
    !view.value?.track ||
    !props.player.lyricsOpen.value ||
    modeSheetOpen.value ||
    uncollect.value ||
    props.commentComposer.current.value ||
    (event.target instanceof Element &&
      event.target.closest('input, textarea, [contenteditable]:not([contenteditable=false])'))
  )
    return;
  const text = vinylCopyText(root, window.getSelection());
  if (!text) return;
  event.clipboardData.setData('text/plain', text);
  event.preventDefault();
}
function clearOwnedVinylSelection() {
  resetLyricSelectionPointer();
  const selection = window.getSelection();
  if (panel.value && vinylSelection(panel.value, selection)) selection?.removeAllRanges();
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
  if (manual.value || commentsVisible.value || selectingPointer !== undefined) return;
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
watch(
  [currentEpoch, currentPlayId, displayMode, () => props.activeDetailTab, commentsVisible],
  clearOwnedVinylSelection,
);
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
  document.addEventListener('copy', copyVinylSelection);
  window.addEventListener('pointerup', endLyricSelection, true);
  window.addEventListener('pointercancel', endLyricSelection, true);
  window.addEventListener('blur', resetLyricSelectionPointer);
  previous = document.activeElement as HTMLElement;
  previousEpoch = view.value?.epoch;
  close.value?.focus();
  if (commentsVisible.value) commentPane.value?.focusBack();
  void scroll();
});
onUnmounted(() => {
  document.removeEventListener('copy', copyVinylSelection);
  window.removeEventListener('pointerup', endLyricSelection, true);
  window.removeEventListener('pointercancel', endLyricSelection, true);
  window.removeEventListener('blur', resetLyricSelectionPointer);
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
    ref="panel"
    class="lyrics-panel"
    :data-vinyl-mode="displayMode"
    :data-vinyl-tab="commentsVisible ? 'comments' : activeDetailTab"
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
          <div class="vinyl-info" :class="{ 'has-track-alias': classicTitleAlias }">
            <div
              v-if="displayMode !== 'lyric'"
              class="vinyl-record"
              :data-cover-state="cover.state.value"
            >
              <img
                v-if="cover.showImage.value"
                v-show="cover.loaded.value"
                :key="cover.generation.value"
                :src="cover.source.value!"
                :data-cover-generation="cover.generation.value"
                :alt="voiceMetadata ? '节目封面' : '专辑封面'"
                referrerpolicy="no-referrer"
                @load="cover.load"
                @error="cover.error"
              /><Icon v-if="!cover.loaded.value" name="music" data-cover-placeholder />
            </div>
            <h1 :title="displayedTitle" data-vinyl-copy-text>
              {{ displayedTitle }}
            </h1>
            <p
              v-if="classicTitleAlias"
              class="vinyl-track-alias"
              :title="classicTitleAlias"
              data-vinyl-copy-text
            >
              {{ classicTitleAlias }}
            </p>
            <ul
              v-if="voiceMetadata"
              class="vinyl-voice-meta"
              aria-label="节目资料"
              data-voice-detail
              data-non-selectable
            >
              <li v-if="voiceMetadata.creator" class="voice-creator">
                <span>主播：</span>
                <button
                  v-if="creatorTarget"
                  type="button"
                  class="text-button vinyl-credit"
                  data-voice-creator
                  :aria-label="`查看主播：${voiceMetadata.creator.name}`"
                  :title="voiceMetadata.creator.name"
                  @click="openVoiceCreator"
                >
                  {{ voiceMetadata.creator.name }}
                </button>
                <span v-else :title="voiceMetadata.creator.name" data-voice-creator-name>
                  {{ voiceMetadata.creator.name }}
                </span>
              </li>
              <li class="voice-release" data-voice-release>
                <span>发布时间：</span
                ><time v-if="voiceMetadata.releaseDate">{{ voiceMetadata.releaseDate }}</time>
                <span v-else>—</span>
              </li>
            </ul>
            <p v-else-if="view?.track" class="vinyl-credits" aria-label="歌手" data-non-selectable>
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
            <small
              v-if="displayMode === 'classic' && view?.track && !voiceMetadata"
              class="vinyl-album"
              data-non-selectable
            >
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
            <small
              v-if="displayMode === 'classic' && source"
              class="vinyl-source"
              data-non-selectable
            >
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
            <span v-if="view?.timeline?.mode === 'trial'" class="trial-badge" data-non-selectable
              >当前为试听片段</span
            >
            <p v-if="displayMode === 'cover'" class="vinyl-cover-line" data-vinyl-copy-text>
              {{ coverLine }}
            </p>
          </div>
          <div v-if="displayMode !== 'cover'" class="lyrics-column">
            <div
              v-if="voiceMetadata?.description || podcastTarget || recommendedPodcasts"
              ref="voiceCards"
              class="voice-playback-cards"
              :class="{ 'voice-playback-cards-only': !lines.length }"
              @wheel.stop.passive
            >
              <VoicePodcastCard
                v-if="voiceMetadata?.radio && podcastTarget"
                :radio="voiceMetadata.radio"
                :target="podcastTarget"
                :episode-cover="view?.track?.coverUrl"
                :signed-in="account.status === 'authenticated'"
                :control="podcastFollow.control(podcastTarget.radioId)"
                :refreshing="podcastFollow.refreshing.value"
                :status="podcastFollow.status.value"
                @open="emit('voicePodcast', $event)"
                @activate="activateVoicePodcast"
              />
              <VoiceDescriptionCard
                v-if="voiceMetadata?.description"
                :key="`${currentEpoch}:${currentPlayId}`"
                :text="voiceMetadata.description"
              />
              <VoiceRecommendationsCard
                v-if="recommendedPodcasts"
                :key="recommendedPodcasts.receipt"
                :page="recommendedPodcasts"
                :epoch="account.epoch"
                :paging="voiceRecommendations.paging.value"
                @next="voiceRecommendations.next"
                @open="openVoiceRecommendation"
              />
            </div>
            <div
              v-if="wikiAvailable"
              ref="tabList"
              class="vinyl-detail-tabs"
              role="tablist"
              aria-label="播放详情"
              data-non-selectable
              @keydown="detailKey"
            >
              <button
                v-for="tab in tabs"
                :id="'vinyl-' + tab + '-tab'"
                :key="tab"
                type="button"
                role="tab"
                :data-vinyl-detail-tab="tab"
                :data-vinyl-lyrics-tab="tab === 'lyrics' ? '' : undefined"
                :data-vinyl-wiki-tab="tab === 'wiki' ? '' : undefined"
                :data-vinyl-similar-tab="tab === 'similar' ? '' : undefined"
                :aria-controls="'vinyl-' + tab + '-content'"
                :aria-selected="activeDetailTab === tab"
                :tabindex="activeDetailTab === tab ? 0 : -1"
                @click="detailTab = tab"
              >
                {{ tabLabel[tab] }}
              </button>
            </div>
            <div
              v-show="
                activeDetailTab === 'lyrics' &&
                (!(voiceMetadata?.description || podcastTarget || recommendedPodcasts) ||
                  lines.length)
              "
              id="vinyl-lyrics-content"
              class="vinyl-detail-content"
              :role="wikiAvailable ? 'tabpanel' : undefined"
              :aria-labelledby="wikiAvailable ? 'vinyl-lyrics-tab' : undefined"
            >
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
                    @pointerdown="beginLyricSelection"
                    @wheel.stop.passive="pauseLyricFollow"
                    @touchmove.passive="pauseLyricFollow"
                    @scroll.passive="selectBrowsedLine"
                  >
                    <div class="lyric-padding" />
                    <div
                      v-for="(line, index) in lines"
                      :key="`${line.target?.epoch}:${line.target?.playId}:${index}`"
                      class="lyric-line"
                      :class="{ current: index === active, browsed: index === browsedIndex }"
                      :data-line="index"
                      role="button"
                      :aria-disabled="!seekable(line)"
                      :tabindex="seekable(line) ? 0 : -1"
                      @click="clickLyricLine($event, line)"
                      @keydown="lyricLineKey($event, line)"
                    >
                      <span
                        v-if="timedWords && lyrics.wordLines[index]?.words.length"
                        class="word-line"
                        data-lyric-text
                        ><span
                          v-for="(word, wi) in lyrics.wordLines[index]?.words"
                          :key="wi"
                          class="timed-word"
                          :style="{ '--word-progress': wordProgress(word) }"
                          >{{ word.text }}</span
                        ></span
                      >
                      <span v-else :data-lyric-text="line.lyric ? '' : undefined">{{
                        line.lyric || '♪'
                      }}</span>
                      <small v-if="displayedAuxiliary[index]" data-lyric-text>{{
                        displayedAuxiliary[index]
                      }}</small>
                    </div>
                    <p
                      v-for="credit in contributors"
                      :key="`${currentEpoch}:${currentPlayId}:${credit.kind}`"
                      class="lyric-contributor"
                      :data-lyric-credit="credit.kind"
                      data-non-selectable
                    >
                      {{ credit.label
                      }}<button
                        v-if="credit.target"
                        type="button"
                        class="lyric-contributor-link"
                        @click="emit('lyricContributor', credit.target)"
                      >
                        {{ credit.nickname }}</button
                      ><span v-else>{{ credit.nickname }}</span>
                    </p>
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
                  <button
                    v-if="manual"
                    ref="resumeButton"
                    class="text-button"
                    @click="followLyrics"
                  >
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
            <div
              v-if="wikiAvailable && view?.playId"
              v-show="activeDetailTab === 'wiki'"
              id="vinyl-wiki-content"
              class="vinyl-detail-content vinyl-detail-wiki"
              role="tabpanel"
              aria-labelledby="vinyl-wiki-tab"
            >
              <div
                v-if="activeDetailTab === 'wiki'"
                ref="wikiScroll"
                class="vinyl-detail-wiki-scroll"
                tabindex="0"
                aria-label="百科内容"
                @wheel.stop.passive
              >
                <VinylWikiPane
                  :play-id="view.playId"
                  :state="wikiModel.state.value"
                  :card="wikiModel.card.value"
                  embedded
                  @retry="wikiModel.retry"
                />
                <VinylSimilarPane
                  v-if="recommendationPlacement === 'wiki'"
                  v-bind="recommendationProps"
                  embedded
                  @play="similarModel.select"
                  @retry="emit('retryRecommendation', $event)"
                  @expand="similarModel.expand"
                  @open-playlist="emit('relatedPlaylist', $event)"
                  @open-artist="emit('catalog', { kind: 'artist', id: $event })"
                />
              </div>
            </div>
            <div
              v-if="wikiAvailable && recommendationPlacement === 'standalone' && view?.playId"
              v-show="activeDetailTab === 'similar'"
              id="vinyl-similar-content"
              class="vinyl-detail-content vinyl-detail-similar"
              role="tabpanel"
              aria-labelledby="vinyl-similar-tab"
              @wheel.stop.passive
            >
              <VinylSimilarPane
                v-if="activeDetailTab === 'similar'"
                v-bind="recommendationProps"
                @play="similarModel.select"
                @retry="emit('retryRecommendation', $event)"
                @expand="similarModel.expand"
                @open-playlist="emit('relatedPlaylist', $event)"
                @open-artist="emit('catalog', { kind: 'artist', id: $event })"
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
          :write-target="commentWriteTarget"
          @back="commentsOpen = false"
          @full="emit('fullComments', false)"
          @hot="emit('fullComments', true)"
          @compose="commentComposer.open"
          @report-expired="emit('commentReportExpired')"
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
    <Teleport to="body">
      <ModalTransition :return-focus="() => uncollectOpener" :fallback-focus="panelCloseFocus">
        <VoicePodcastUncollectDialog
          v-if="uncollect"
          @close="uncollect = undefined"
          @confirm="confirmVoicePodcastUncollect"
        />
      </ModalTransition>
      <ModalTransition :return-focus="commentOpener" :fallback-focus="panelCloseFocus">
        <VinylCommentDialog
          v-if="commentComposer.current.value"
          :key="commentComposer.current.value.id"
          :opening="commentComposer.current.value"
          @close="commentComposer.close"
          @independent="independentComment"
        />
      </ModalTransition>
    </Teleport>
  </section>
</template>
