<script setup lang="ts">
import type { SearchPodcastRowGesture } from '../../services/player';
import type { SearchVoiceMenuTarget } from '../../services/search';
import type {
  TrackCurrentToggle,
  TrackMenuRequest,
  TrackPendingToggle,
} from '../../services/trackMenu';
import type { ArtistCardRequest } from '../../services/artistCards';
import type { AlbumCardRequest } from '../../services/catalogCards';
import type { PlaylistLinkAction } from '../../services/playlistMenu';
import { computed, nextTick, onBeforeUnmount, ref, shallowRef, watch } from 'vue';
import { advanceSearchPage, type SearchPageStep } from '../../services/searchPagination';
import type { SearchViewportMemory } from '../../services/searchViewport';
import {
  captureSearchPresentationViewport,
  restoreSearchPresentationViewport,
} from '../../services/searchPresentationViewport';
import SearchCardGrid from './SearchCardGrid.vue';
import type { SearchTableKind } from '../../services/searchTableColumns';
import SearchTrackCard from './SearchTrackCard.vue';
import type { AccountView, PlaylistView, TrackView } from '../../services/contracts';
import {
  comprehensivePlayAllTracks,
  displayedSearchCategories,
  searchExplanation,
  type SearchCategory,
  type SearchChannel,
  type SearchHotTarget,
  type SearchHotPlaybackTarget,
  type SearchItem,
  type SearchReasonTarget,
  type SearchSection,
} from '../../services/search';
import { useLyricWindow } from '../../composables/useLyricWindow';
import { SEARCH_PLAYBACK_ROW_LIMIT, useSearchResults } from '../../composables/useSearchResults';
import { useSearchPodcastFollow } from '../../composables/useSearchPodcastFollow';
import { useMotion } from '../../composables/useMotion';
import { createPageTransition } from '../../services/pageTransition';
import { useCore } from '../../services/core';
import { podcastProgramPublicLink, type PodcastProgramShare } from '../../services/podcast';
import ModalTransition from '../ModalTransition.vue';
import TrackShareDialog from '../TrackShareDialog.vue';
import TrackTable from '../TrackTable.vue';
import TrackRow from '../TrackRow.vue';
import SearchLyricExtra from './SearchLyricExtra.vue';
import { waitForViewportFrames } from '../../services/viewportLayout';
import Icon from '../Icon.vue';
import SearchResultCard from './SearchResultCard.vue';
import SearchHotExplanationCard from './SearchHotExplanationCard.vue';
import GuestSearchResult from './GuestSearchResult.vue';
import {
  searchCardIsCurrent,
  searchVoiceLinkSection,
  searchVoiceProgram,
  type SearchPendingVoiceToggle,
  type SearchQueueRequest,
  type SearchVoiceLinkRequest,
} from '../../services/searchCards';
import type { PlayerOrigin, PlaybackStatus, SearchResultPlayback } from '../../services/player';
const motion = useMotion();
const panelTransition = createPageTransition(() => motion.reduced.value, undefined, {
  name: 'search-results',
  deadlineMs: 320,
});
onBeforeUnmount(panelTransition.dispose);
const props = defineProps<{
  account: AccountView;
  keyword: string;
  category?: SearchCategory;
  channel?: SearchChannel;
  defaultReceipt?: string;
  correctionReceipt?: string;
  pagination?: SearchPageStep;
  correct?: boolean;
  rewrittenKeyword?: string;
  liked: Set<string>;
  busyLikes: Set<string>;
  playable: boolean;
  queueable: boolean;
  queueDropEnabled: boolean;
  voiceQueueable: boolean;
  cardQueueFeedback: string;
  playlistActionBusy: boolean;
  knownPlaylistIds?: ReadonlySet<string>;
  observedAlbumMembership: ReadonlyMap<string, boolean>;
  currentId?: string;
  currentToggle?: TrackCurrentToggle;
  pendingTrackToggle?: TrackPendingToggle;
  pendingVoiceToggle?: SearchPendingVoiceToggle;
  playbackOrigin?: PlayerOrigin | null;
  playbackState?: PlaybackStatus;
  currentVoiceId?: string;
  memory: SearchViewportMemory;
  detailDeparture: () => boolean;
  searchId?: number;
  guestReady?: boolean;
}>();
const emit = defineEmits<{
  reason: [label: string, target: SearchReasonTarget];
  navigate: [
    category: SearchCategory,
    pagination: SearchPageStep | undefined,
    correct: boolean,
    rewrittenKeyword?: string,
  ];
  search: [keyword: string];
  searchOriginal: [keyword: string, receipt?: string];
  login: [];
  open: [item: SearchItem];
  hotTarget: [target: SearchHotTarget, anchor: HTMLButtonElement];
  hotPlay: [target: SearchHotPlaybackTarget];
  creator: [id: string];
  play: [tracks: TrackView[], index: number, context: SearchResultPlayback];
  playTrack: [track: TrackView, toggleCurrent: boolean];
  toggleCurrent: [target: TrackCurrentToggle, anchor: HTMLButtonElement];
  togglePendingTrack: [target: TrackPendingToggle, anchor?: HTMLButtonElement];
  downloadTrack: [track: TrackView];
  playlistDownload: [playlist: PlaylistView];
  playlistLink: [playlist: PlaylistView, action: PlaylistLinkAction];
  playlistSubscribe: [playlist: PlaylistView];
  collect: [tracks: TrackView[]];
  more: [request: TrackMenuRequest];
  like: [track: TrackView];
  artist: [id: string];
  artistAction: [request: ArtistCardRequest];
  albumAction: [request: AlbumCardRequest];
  album: [id: string];
  mv: [id: string];
  expired: [];
  playCard: [item: SearchItem, gesture?: SearchPodcastRowGesture];
  voiceMenuPlay: [target: SearchVoiceMenuTarget];
  togglePendingVoice: [target: SearchPendingVoiceToggle, anchor: HTMLButtonElement];
  queueCard: [request: SearchQueueRequest];
  queueDrag: [
    event: PointerEvent,
    rows: { receipt: string; trackId: string }[],
    current: () => boolean,
  ];
  queueVoice: [id: string];
  cardComments: [item: SearchItem];
  cardDownload: [item: SearchItem];
}>();
const core = useCore();
const model = useSearchResults(core, () => emit('expired'), props.memory);
const { view, loading, error, loadingMore, moreError, manualMore } = model;
const category = computed(() => props.category ?? 'all'),
  offset = computed(() => props.pagination?.offset ?? 0);
const tableCategories: Partial<Record<SearchCategory, SearchTableKind>> = {
  playlists: 'playlist',
  albums: 'album',
  voices: 'voice',
  podcasts: 'podcast',
};
const tableKind = computed(() => tableCategories[category.value]);
const podcastFollow = useSearchPodcastFollow(
  core,
  () => props.account,
  () => category.value === 'podcasts',
  () => emit('expired'),
);
const nextPage = computed(() =>
  view.value?.more ? advanceSearchPage(props.pagination, view.value.nextOffset) : undefined,
);
const rewrite = computed(() =>
  props.correct === false
    ? undefined
    : (view.value?.correction ?? (category.value !== 'all' ? props.rewrittenKeyword : undefined)),
);
const matchingReminder = computed(() => {
  const reminder = model.correctionReminder.value;
  return reminder && reminder.rewrite === rewrite.value ? reminder : undefined;
});
const correctionParts = computed(() => matchingReminder.value?.parts);
const requestKeyword = computed(() =>
  category.value !== 'all' && props.correct !== false
    ? (props.rewrittenKeyword ?? props.keyword)
    : props.keyword,
);
const matchKeyword = computed(() =>
  category.value === 'all' ? props.keyword : (rewrite.value ?? requestKeyword.value),
);
const lyrics = computed(() => category.value === 'lyrics');
const songWaterfall = computed(() => category.value === 'songs' && props.pagination === undefined);
const playlistWaterfall = computed(
  () => category.value === 'playlists' && props.pagination === undefined,
);
const firstNumberedPage: SearchPageStep = { offset: 0, number: 1, previous: undefined };
const tenItemGrid = computed(
  () => playlistWaterfall.value || ['albums', 'voices', 'podcasts'].includes(category.value),
);
const gridWaterfall = computed(
  () => tenItemGrid.value || ['artists', 'users', 'mvs'].includes(category.value),
);
const waterfall = computed(() => lyrics.value || gridWaterfall.value || songWaterfall.value);
const limit = computed(() =>
  lyrics.value || songWaterfall.value || tenItemGrid.value ? 10 : gridWaterfall.value ? 13 : 30,
);
const visibleTabs = computed(() => displayedSearchCategories(model.tabs.value, category.value));
const selected = computed(() => visibleTabs.value.find((c) => c.key === category.value)!);
const tracks = (section: SearchSection) =>
  section.items.flatMap((i) => (i.kind === 'song' ? [i.track] : []));
const songTableHints = (section: SearchSection) =>
  section.items.flatMap((item) => (item.kind === 'song' ? [item.tableHint] : []));
function startQueueDrop(
  event: PointerEvent,
  section: SearchSection,
  rows: { track: TrackView; index: number }[],
  selectionCurrent: () => boolean,
) {
  if (category.value !== 'songs' || !props.queueDropEnabled || loading.value) return;
  const snapshot = view.value;
  const items = section.items.filter((item) => item.kind === 'song');
  const page = props.searchId;
  const epoch = props.account.epoch;
  const current = () =>
    props.queueDropEnabled &&
    category.value === 'songs' &&
    !loading.value &&
    page === props.searchId &&
    epoch === props.account.epoch &&
    view.value === snapshot &&
    selectionCurrent();
  const tickets = rows.flatMap(({ track, index }) => {
    const item = items[index];
    return item?.track === track && item.queueDropReceipt
      ? [{ receipt: item.queueDropReceipt, trackId: track.id }]
      : [];
  });
  if (tickets.length && tickets.length === rows.length && current())
    emit('queueDrag', event, tickets, current);
}
const originalRecordings = computed(
  () =>
    new Set(
      category.value === 'songs'
        ? (view.value?.sections.flatMap((section) =>
            section.items.flatMap((item) =>
              item.kind === 'song' && item.originalRecording ? [item.track.id] : [],
            ),
          ) ?? [])
        : [],
    ),
);
function bestMatch(section: SearchSection) {
  return ['search_block_best_match:', 'search_block_hot_result_core_resources:'].some((prefix) =>
    section.id.startsWith(prefix),
  );
}
const expandedPreview = computed(
  () =>
    view.value?.previewLayout === 'carousel' || view.value?.previewLayout === 'compact_carousel',
);
const compactPreview = computed(
  () =>
    view.value?.previewLayout === 'compact_grid' ||
    view.value?.previewLayout === 'compact_carousel',
);
function expandedBestMatch(section: SearchSection) {
  return category.value === 'all' && expandedPreview.value && bestMatch(section);
}
function previewItems(section: SearchSection) {
  const items = section.items.filter((i) => i.kind !== 'song');
  if (category.value !== 'all') return items;
  if (bestMatch(section))
    return items
      .filter((item) => item.kind !== 'related')
      .slice(0, expandedBestMatch(section) ? 3 : 2);
  if (carousel(section)) return items.slice(0, 10);
  return section.category && !['voices', 'all'].includes(section.category)
    ? items.slice(0, 6)
    : items;
}
const previewPositions = ref<Record<string, number>>({});
function carousel(section: SearchSection) {
  if (!expandedPreview.value || !oneRowPreview(section)) return;
  return {
    label: section.title,
    compact: compactPreview.value,
    position: previewPositions.value[section.id] ?? 0,
  };
}
function previewPosition(section: SearchSection, index: number) {
  previewPositions.value[section.id] = index;
  checkpoint();
}
function oneRowPreview(section: SearchSection) {
  return (
    category.value === 'all' &&
    !bestMatch(section) &&
    ['playlists', 'artists', 'albums', 'podcasts', 'mvs', 'users'].includes(section.category ?? '')
  );
}
function showTrackMenu(request: TrackMenuRequest) {
  emit('more', { ...request, nextOrigin: 'search' });
}
function showSelectedTrackMenu(
  section: SearchSection,
  request: TrackMenuRequest,
  anchorIndex: number,
  rows: { track: TrackView; index: number }[],
  selectionCurrent: () => boolean,
) {
  const owner = panel.value;
  const snapshot = view.value;
  const epoch = props.account.epoch;
  const visit = props.searchId;
  const current = () =>
    props.account.status === 'authenticated' &&
    props.account.epoch === epoch &&
    props.searchId === visit &&
    category.value === 'songs' &&
    !loading.value &&
    view.value === snapshot &&
    panel.value === owner &&
    !!owner?.isConnected &&
    !owner.closest('[inert], .page-leave-active, .search-results-leave-active') &&
    selectionCurrent();
  if (!current()) return;
  const items = section.items.filter((item) => item.kind === 'song');
  const tickets = rows.map(({ track, index }) => {
    const item = items[index];
    if (item?.track !== track || !item.queueDropReceipt || !item.groupNextRight) return;
    return { receipt: item.queueDropReceipt, trackId: track.id };
  });
  const anchor = tickets[rows.findIndex((row) => row.index === anchorIndex)];
  const action =
    anchor && tickets.every((ticket) => !!ticket) ? { anchor, rows: tickets } : undefined;
  const allCopyrightBlocked = rows.every(
    ({ index }) => items[index]?.groupNextRight === 'copyright_blocked',
  );
  const downloadRows = rows.map(({ track, index }) => {
    const item = items[index];
    if (item?.track !== track || !item.groupDownload) return;
    return { receipt: item.groupDownload.receipt, trackId: track.id };
  });
  const downloadAnchor = downloadRows[rows.findIndex((row) => row.index === anchorIndex)];
  const download =
    downloadAnchor && downloadRows.every((row) => !!row)
      ? { anchor: downloadAnchor, rows: downloadRows }
      : undefined;
  const allDownloadsBlocked = rows.every(
    ({ index }) => items[index]?.groupDownload?.right === 'copyright_blocked',
  );
  emit('more', {
    ...request,
    searchGroup: {
      epoch,
      action,
      allCopyrightBlocked,
      live: current,
      download,
      allDownloadsBlocked,
    },
  });
}
function playComprehensiveAll(section: SearchSection) {
  const rows = comprehensivePlayAllTracks(section);
  const first = rows.findIndex((track) => !track.missing);
  if (first >= 0) emit('play', rows, first, 'comprehensive_all');
}
function playTrack(track: TrackView | undefined, toggleCurrent = false) {
  if (track && !track.missing) emit('playTrack', track, toggleCurrent);
}
const songs = computed(() => view.value?.sections.flatMap(tracks) ?? []);
const count = computed(() => view.value?.sections.reduce((n, s) => n + s.items.length, 0) ?? 0);
const explanation = computed(() => searchExplanation(view.value, category.value, count.value > 0));
const hotspot = computed(() => (category.value === 'all' ? view.value?.hotExplanation : undefined));
const hotPending = computed(() => {
  const target = hotspot.value?.playTarget;
  if (target?.kind === 'song' && props.pendingTrackToggle?.trackId === target.id)
    return props.pendingTrackToggle;
  if (target?.kind === 'voice' && props.pendingVoiceToggle?.voiceId === target.id)
    return props.pendingVoiceToggle;
});
function playHotTarget(target: SearchHotPlaybackTarget, anchor: HTMLButtonElement) {
  const pending = hotPending.value;
  if (pending) {
    if ('trackId' in pending) emit('togglePendingTrack', pending, anchor);
    else emit('togglePendingVoice', pending, anchor);
    return;
  }
  emit('hotPlay', target);
}

const expandedLyrics = ref(new Set<string>());
function toggleLyric(key: string) {
  const next = new Set(expandedLyrics.value);
  if (!next.delete(key)) next.add(key);
  expandedLyrics.value = next;
  void nextTick(() => checkpoint());
}
const panel = ref<HTMLElement>(),
  tabs = ref<HTMLElement>();
const retryButton = ref<HTMLButtonElement>();
const moreButton = ref<HTMLButtonElement>(),
  moreFooter = ref<HTMLElement>();
const searchSongTables = ref<InstanceType<typeof TrackTable>[]>([]);
const searchCardGrids = ref<InstanceType<typeof SearchCardGrid>[]>([]);
const lyricBody = ref<HTMLElement>();
const lyricItems = computed(() =>
  lyrics.value ? (view.value?.sections[0]?.items.filter((item) => item.kind === 'song') ?? []) : [],
);
const lyricKey = (index: number, id: string) => `${index}:${id}`;
const lyricWindow = useLyricWindow(panel, lyricBody, () =>
  lyricItems.value.map((item, index) =>
    expandedLyrics.value.has(lyricKey(index, item.track.id))
      ? 90 + Math.max(3, item.excerpt.length) * 18
      : 144,
  ),
);
const renderedLyrics = computed(() =>
  lyricWindow.rendered.value.flatMap((index) => {
    const item = lyricItems.value[index];
    return item ? [{ item, index, key: lyricKey(index, item.track.id) }] : [];
  }),
);
function lyricFocus(event: FocusEvent) {
  const row = (event.target as Element | null)?.closest<HTMLElement>('[data-lyric-index]');
  if (row) lyricWindow.focus(Number(row.dataset.lyricIndex));
}
function lyricBlur(event: FocusEvent) {
  if (!(event.relatedTarget instanceof Node) || !lyricBody.value?.contains(event.relatedTarget))
    lyricWindow.focus();
}
function open(item: SearchItem) {
  if (item.kind === 'related') emit('search', item.keyword);
  else if (item.kind !== 'user' || !item.user.unavailable) emit('open', item);
}
function choose(key: SearchCategory, fromKeyboard = false) {
  if (key !== category.value) turn(key);
  if (fromKeyboard)
    void nextTick(() =>
      tabs.value?.querySelector<HTMLButtonElement>(`[data-category="${key}"]`)?.focus(),
    );
}
function turn(key: SearchCategory, pagination?: SearchPageStep) {
  emit(
    'navigate',
    key,
    pagination,
    props.correct ?? true,
    key === 'all' ? undefined : rewrite.value,
  );
}
function tabKey(event: KeyboardEvent, index: number) {
  const categories = visibleTabs.value;
  let next = index;
  if (event.key === 'ArrowRight') next = (index + 1) % categories.length;
  else if (event.key === 'ArrowLeft') next = (index + categories.length - 1) % categories.length;
  else if (event.key === 'Home') next = 0;
  else if (event.key === 'End') next = categories.length - 1;
  else return;
  event.preventDefault();
  choose(categories[next]!.key, true);
}
let restoring = false,
  loadGeneration = 0,
  frame = 0;
let scrollDeadline: ReturnType<typeof setTimeout> | undefined;
let activeCategory = '';
let observer: ResizeObserver | undefined;
let hotDeparture: HTMLButtonElement | undefined;
interface VoiceLinkOwner {
  epoch: string;
  generation: number;
  panel: HTMLElement;
  sectionId: string;
  request: SearchVoiceLinkRequest;
}
const voiceLinkOwner = shallowRef<VoiceLinkOwner>();
const voiceLinkShare = shallowRef<{ program: PodcastProgramShare; canCopy: () => boolean }>();
const voiceLinkStatus = ref('');
const voiceLinkRestoreFocus = ref(false);
let voiceLinkAnchor: HTMLElement | null = null;
function voiceLinkCurrent(owner: VoiceLinkOwner) {
  if (
    props.account.status !== 'authenticated' ||
    props.account.epoch !== owner.epoch ||
    loadGeneration !== owner.generation ||
    loading.value ||
    panel.value !== owner.panel ||
    !owner.panel.isConnected ||
    owner.panel.closest('[inert], .page-leave-active, .search-results-leave-active')
  )
    return false;
  const section = view.value?.sections.find((section) => section.id === owner.sectionId);
  return !!section && !!searchVoiceProgram(category.value, section, owner.request.item);
}
function clearVoiceLink(restoreFocus = false) {
  voiceLinkRestoreFocus.value = restoreFocus;
  voiceLinkOwner.value = undefined;
  voiceLinkShare.value = undefined;
  voiceLinkStatus.value = '';
}
watch(
  () => !!voiceLinkOwner.value && voiceLinkCurrent(voiceLinkOwner.value),
  (current) => {
    if (!current && voiceLinkOwner.value) clearVoiceLink();
  },
  { flush: 'sync' },
);
async function activateVoiceLink(section: SearchSection, request: SearchVoiceLinkRequest) {
  if (!panel.value || !request.anchor || !panel.value.contains(request.anchor)) return;
  const owner = {
    epoch: props.account.epoch,
    generation: loadGeneration,
    panel: panel.value,
    sectionId: section.id,
    request,
  };
  const program = searchVoiceProgram(category.value, section, request.item);
  if (!program || !voiceLinkCurrent(owner)) return;
  const link = podcastProgramPublicLink(program.id);
  if (!link) return;
  clearVoiceLink();
  voiceLinkOwner.value = owner;
  const current = () => voiceLinkOwner.value === owner && voiceLinkCurrent(owner);
  if (request.action === 'share') {
    voiceLinkAnchor = request.anchor;
    voiceLinkShare.value = { program, canCopy: current };
    return;
  }
  try {
    await core.copyText(link);
    if (current()) voiceLinkStatus.value = '链接已复制';
  } catch {
    if (current()) voiceLinkStatus.value = '复制失败，请重试';
  }
}
function trackFocusKey(row?: HTMLElement | null) {
  return row?.dataset.trackId
    ? `song-row:${row.getAttribute('aria-rowindex')}:${row.dataset.trackId}`
    : undefined;
}
function checkpoint(focus = false, explicit?: HTMLElement, freshPreviews = false) {
  if (freshPreviews) {
    previewPositions.value = {};
    model.resetPreviewPages();
  }
  if (!panel.value || (!explicit && (restoring || loading.value))) return;
  const active =
    explicit && panel.value.contains(explicit)
      ? explicit
      : focus && document.activeElement instanceof HTMLElement
        ? document.activeElement
        : undefined;
  const card = active?.closest<HTMLElement>('[data-result-key]');
  const row =
    activeCategory === 'songs'
      ? active?.closest<HTMLElement>('.track-row[data-track-id]')
      : undefined;
  const key = card?.dataset.resultKey ?? trackFocusKey(row);
  model.savePosition({
    top: panel.value.scrollTop,
    tracks: panel.value.querySelector('.track-scroll')?.scrollTop ?? 0,
    focus: key,
    focusSection: card?.closest<HTMLElement>('[data-result-section]')?.dataset.resultSection,
    focusAction: active?.dataset.resultAction,
    previewPositions: { ...previewPositions.value },
    expandedLyrics: [...expandedLyrics.value],
    lyricAnchor: activeCategory === 'lyrics' ? lyricWindow.anchor() : undefined,
  });
}
function openHotTarget(target: SearchHotTarget, anchor: HTMLButtonElement) {
  if (target.kind !== 'song' && target.kind !== 'voice') {
    // A user may activate the visible CTA before scroll restoration settles.
    // The outgoing control is more specific than the later top-bar focus.
    hotDeparture = anchor;
    checkpoint(true, anchor);
  }
  emit('hotTarget', target, anchor);
}
function maybeMore(songScroll?: HTMLElement) {
  const signedSongScroller = songWaterfall.value && !props.guestReady;
  const element = signedSongScroller ? songScroll : panel.value;
  if (songWaterfall.value && (!element || element.scrollTop <= 0)) return;
  // An append updates the model before Vue paints the new spacer. Do not
  // treat the previous page's bottom as the end of the newly loaded list.
  if (
    lyrics.value &&
    !props.guestReady &&
    (!lyricBody.value ||
      lyricWindow.offsets.value.length !== lyricItems.value.length + 1 ||
      Math.abs(lyricBody.value.clientHeight - lyricWindow.height.value) > 1)
  )
    return;
  if (
    !restoring &&
    waterfall.value &&
    count.value > 0 &&
    element &&
    view.value?.more &&
    !moreError.value &&
    !manualMore.value &&
    element.scrollHeight - element.scrollTop - element.clientHeight <
      (lyrics.value || songWaterfall.value ? 60 : 180)
  )
    void model.more();
}
function scroll(event: Event) {
  checkpoint();
  if (lyrics.value) lyricWindow.schedule();
  const target = event.target;
  const songScroll =
    songWaterfall.value &&
    !props.guestReady &&
    !restoring &&
    target instanceof HTMLElement &&
    target.classList.contains('track-scroll')
      ? target
      : undefined;
  if (songWaterfall.value && !props.guestReady && !songScroll) return;
  const generation = loadGeneration;
  if (!frame) {
    const update = () => {
      cancelAnimationFrame(frame);
      clearTimeout(scrollDeadline);
      frame = 0;
      if (generation === loadGeneration && (!songScroll || panel.value?.contains(songScroll)))
        maybeMore(songScroll);
    };
    frame = requestAnimationFrame(update);
    scrollDeadline = setTimeout(update, 64);
  }
}
watch(
  () => view.value?.more,
  (more) => {
    if (more || !moreButton.value || document.activeElement !== moreButton.value) return;
    const generation = loadGeneration;
    void nextTick(() => {
      if (generation === loadGeneration && document.activeElement === document.body) {
        moreFooter.value?.focus({ preventScroll: true });
        moreFooter.value?.scrollIntoView({ block: 'nearest' });
      }
    });
  },
);
async function restore() {
  if (
    !restoring ||
    loading.value ||
    !panel.value ||
    panel.value.classList.contains('search-results-leave-active')
  )
    return;
  const generation = loadGeneration;
  const navigationFocus = document.activeElement;
  expandedLyrics.value = new Set(model.position.value.expandedLyrics ?? []);
  previewPositions.value = { ...model.position.value.previewPositions };
  await nextTick();
  await waitForViewportFrames(2);
  if (
    generation !== loadGeneration ||
    !panel.value ||
    panel.value.classList.contains('search-results-leave-active')
  )
    return;
  const position = model.position.value;
  if (lyrics.value && position.lyricAnchor) lyricWindow.restore(position.lyricAnchor);
  else panel.value.scrollTop = position.top;
  if (lyrics.value && position.focus?.startsWith('lyric:')) {
    const index = Number(position.focus.split(':')[1]);
    if (Number.isInteger(index)) lyricWindow.focus(index);
  }
  if (category.value === 'songs') searchSongTables.value[0]?.restoreScroll(position.tracks);
  if (category.value === 'playlists')
    for (const grid of searchCardGrids.value) grid.reconcileViewport();
  await waitForViewportFrames(2);
  if (generation !== loadGeneration || !panel.value) return;
  if (position.focus) {
    const section = position.focusSection
      ? [...panel.value.querySelectorAll<HTMLElement>('[data-result-section]')].find(
          (node) => node.dataset.resultSection === position.focusSection,
        )
      : undefined;
    const card = [
      ...(section ?? panel.value).querySelectorAll<HTMLElement>(
        '[data-result-key], .track-row[data-track-id]',
      ),
    ].find(
      (node) =>
        node.dataset.resultKey === position.focus ||
        (category.value === 'songs' && trackFocusKey(node) === position.focus),
    );
    if (card && !card.closest('[inert]')) {
      const controls = [...card.querySelectorAll<HTMLButtonElement>('button:not(:disabled)')];
      (
        controls.find(
          (n) => position.focusAction && n.dataset.resultAction === position.focusAction,
        ) ?? controls[0]
      )?.focus({ preventScroll: true });
    } else {
      const rail = card?.closest('.search-carousel') ?? section?.querySelector('.search-carousel');
      const active = document.activeElement;
      // A reset may put the returning card on an inert page. Keep focus in
      // its rail unless the user already moved to another control.
      if (
        active === document.body ||
        (active === navigationFocus && active?.matches('.back-button'))
      )
        rail
          ?.querySelector<HTMLButtonElement>(
            '[data-grid-index]:not([inert]) .search-card-open:not(:disabled)',
          )
          ?.focus({ preventScroll: true });
    }
  }
  restoring = false;
  maybeMore();
}
async function load(reuse = false) {
  clearVoiceLink();
  restoring = true;
  cancelAnimationFrame(frame);
  clearTimeout(scrollDeadline);
  frame = 0;
  const generation = ++loadGeneration;
  activeCategory = category.value;
  previewPositions.value = {};
  await model.open(
    {
      keyword: requestKeyword.value,
      category: category.value,
      offset: offset.value,
      limit: limit.value,
      correct: props.correct ?? true,
      channel: props.channel,
    },
    reuse,
    props.searchId ?? 0,
    props.keyword,
    props.defaultReceipt,
    props.correctionReceipt,
  );
  if (generation === loadGeneration) void restore();
}
async function retry() {
  const owner = panel.value;
  const owned = document.activeElement === retryButton.value;
  if (owned) owner?.focus({ preventScroll: true });
  const request = load();
  const generation = loadGeneration;
  await request;
  await nextTick();
  if (
    owned &&
    generation === loadGeneration &&
    owner === panel.value &&
    document.activeElement === owner &&
    error.value
  )
    retryButton.value?.focus({ preventScroll: true });
}
watch(
  panel,
  (element) => {
    observer?.disconnect();
    if (element) {
      observer = new ResizeObserver(() => maybeMore());
      observer.observe(element);
      void restore();
    }
  },
  { flush: 'post' },
);
watch(lyricWindow.height, () => maybeMore(), { flush: 'post' });
watch(
  () => view.value,
  () => {
    if (!restoring) void nextTick(() => maybeMore());
  },
);
watch(model.presentationChange, () => {
  const element = panel.value;
  if (!element || restoring || category.value !== 'all') return;
  const generation = loadGeneration;
  const saved = captureSearchPresentationViewport(element);
  void nextTick(() => {
    if (generation === loadGeneration && panel.value === element && !restoring)
      restoreSearchPresentationViewport(element, saved);
  });
});
watch(
  () => [props.account.epoch, props.account.status, props.guestReady],
  () => model.connect(props.account, props.guestReady),
  { immediate: true },
);
watch(
  () => [
    props.account.epoch,
    props.account.status,
    props.guestReady,
    props.keyword,
    requestKeyword.value,
    category.value,
    offset.value,
    limit.value,
    props.correct,
    props.channel,
    props.defaultReceipt,
    props.correctionReceipt,
    props.searchId,
  ],
  () => {
    if (activeCategory)
      checkpoint(true, undefined, activeCategory === 'all' && category.value !== 'all');
    hotDeparture = undefined;
    void load(true);
  },
  { immediate: true },
);
onBeforeUnmount(() => {
  clearVoiceLink();
  checkpoint(true, hotDeparture, activeCategory === 'all' && props.detailDeparture());
  loadGeneration++;
  observer?.disconnect();
  cancelAnimationFrame(frame);
  clearTimeout(scrollDeadline);
  model.dispose();
});
</script>
<template>
  <section class="search-results-page">
    <div class="search-heading-copy">
      <h1
        :class="{ 'sr-only': correctionParts }"
        :aria-label="`搜索「${keyword}」`"
        :data-tooltip="correctionParts ? undefined : keyword"
      >
        {{ keyword }}
      </h1>
      <div v-if="correctionParts" class="search-correction search-correction-fragments">
        <template v-for="(part, index) in correctionParts" :key="index">
          <button
            v-if="part.highlight"
            type="button"
            class="text-button"
            :aria-label="`仍然搜索“${keyword}”`"
            @click="emit('searchOriginal', keyword, matchingReminder?.receipt)"
          >
            {{ part.text }}
          </button>
          <span v-else>{{ part.text }}</span>
        </template>
      </div>
      <p aria-live="polite">
        {{
          loading
            ? '正在搜索…'
            : error
              ? '加载失败'
              : view?.total != null
                ? `找到 ${view.total} ${selected.unit}`
                : '综合结果'
        }}
      </p>
      <p v-if="cardQueueFeedback" class="search-voice-next-notice" role="status">
        {{ cardQueueFeedback }}
      </p>
      <p
        v-if="voiceLinkStatus"
        class="search-voice-next-notice"
        role="status"
        data-search-voice-link-status
      >
        {{ voiceLinkStatus }}
      </p>
    </div>
    <div class="search-results-toolbar">
      <div ref="tabs" class="search-result-tabs" role="tablist" aria-label="搜索结果分类">
        <button
          v-for="(tab, i) in visibleTabs"
          :id="`search-tab-${tab.key}`"
          :key="tab.key"
          role="tab"
          :aria-selected="category === tab.key"
          aria-controls="search-result-panel"
          :tabindex="category === tab.key ? 0 : -1"
          :data-category="tab.key"
          @click="choose(tab.key)"
          @keydown="tabKey($event, i)"
        >
          {{ tab.title }}
        </button>
      </div>
      <div v-if="guestReady" class="search-result-actions search-guest-actions">
        <button class="secondary" type="button" @click="emit('login')">登录后播放和收藏</button>
      </div>
      <div v-else-if="category === 'songs'" class="search-result-actions">
        <span id="search-song-batch-scope" class="sr-only">
          {{ songWaterfall ? '仅限本次已加载的单曲' : '仅限当前页的单曲' }}
        </span>
        <button
          class="primary"
          aria-describedby="search-song-batch-scope"
          :disabled="!playable || !songs.some((t) => !t.missing)"
          @click="
            emit(
              'play',
              songs,
              songs.findIndex((t) => !t.missing),
              'song_tab_all',
            )
          "
        >
          <Icon name="play" />播放全部</button
        ><button
          class="secondary"
          aria-describedby="search-song-batch-scope"
          :disabled="!songs.some((t) => !t.missing)"
          @click="
            emit(
              'collect',
              songs.filter((t) => !t.missing),
            )
          "
        >
          <Icon name="plus" />收藏全部
        </button>
      </div>
    </div>
    <div v-if="rewrite && !correctionParts" class="search-correction">
      正在显示“{{ rewrite }}”的结果
      <button class="text-button" @click="emit('searchOriginal', keyword)">
        仍然搜索“{{ keyword }}”
      </button>
    </div>
    <Transition
      :css="false"
      mode="out-in"
      @enter="panelTransition.enter"
      @leave="panelTransition.leave"
      @enter-cancelled="panelTransition.cancel"
      @leave-cancelled="panelTransition.cancel"
      @after-enter="restore"
    >
      <div
        :key="`${searchId}:${keyword}:${category}:${offset}:${limit}:${correct}`"
        id="search-result-panel"
        ref="panel"
        role="tabpanel"
        :aria-labelledby="`search-tab-${category}`"
        :aria-busy="loading"
        class="search-result-panel"
        :class="{
          'search-song-panel': category === 'songs' && !guestReady,
          'search-lyrics-panel': lyrics && !guestReady,
          'search-playlist-panel': category === 'playlists',
          'search-album-panel': category === 'albums',
          'search-voice-panel': category === 'voices',
          'search-podcast-panel': category === 'podcasts',
        }"
        tabindex="0"
        @scroll.capture.passive="scroll"
      >
        <div v-if="loading" class="empty-state" role="status">正在搜索…</div>
        <div v-else-if="error" class="empty-state">
          <p role="alert">{{ error }}</p>
          <button ref="retryButton" type="button" class="secondary" @click="retry">重新加载</button>
        </div>
        <template v-else-if="view">
          <p v-if="podcastFollow.status.value" class="search-result-notice" role="status">
            {{ podcastFollow.status.value }}
          </p>
          <p v-if="explanation.notice" class="search-result-notice" role="status">
            {{ explanation.notice }}
          </p>
          <SearchHotExplanationCard
            v-if="hotspot"
            :explanation="hotspot"
            :playable="hotPending ? !hotPending.disabled : playable"
            :detail-ready="playable"
            :playing="
              hotPending?.pause ??
              ((playbackState === 'playing' || playbackState === 'buffering') &&
                !!hotspot.playTarget &&
                (hotspot.playTarget.kind === 'song'
                  ? currentId === hotspot.playTarget.id
                  : currentVoiceId === hotspot.playTarget.id))
            "
            @play="playHotTarget"
            @open="openHotTarget"
          />
          <p v-if="view.unavailableCount" class="search-result-notice">
            有 {{ view.unavailableCount }} 项结果暂无法展示。
          </p>
          <div v-if="!count && !hotspot" class="empty-state">
            <Icon name="search" />
            <p v-if="explanation.emptyTitle">{{ explanation.emptyTitle }}</p>
            <p v-else-if="view.more">这一页暂无可展示的结果</p>
            <p v-else-if="view.unavailableCount">暂无可展示的结果</p>
            <p v-else-if="gridWaterfall" class="search-empty-category">
              咱搜不到关于“<span class="search-empty-keyword">{{ matchKeyword }}</span
              >”的任何{{ selected.title }}呀！
            </p>
            <p v-else>没有找到相关{{ selected.title === '综合' ? '内容' : selected.title }}</p>
            <small>{{ view.more ? '可继续查看后续结果' : '试试其他关键词，或切换分类' }}</small>
          </div>
          <section
            v-for="section in view.sections"
            :key="section.id"
            class="search-result-section"
            :data-result-section="section.id"
            :class="{
              'search-result-tracks': section.items.some((i) => i.kind === 'song'),
              'search-guest-section': guestReady,
              'search-guest-list': guestReady && (category === 'songs' || lyrics),
              'search-best-section': bestMatch(section),
              'search-best-expanded': expandedBestMatch(section),
              'search-best-compact':
                expandedBestMatch(section) && previewItems(section).length === 3,
            }"
            :style="
              expandedBestMatch(section)
                ? { '--search-best-columns': Math.max(1, previewItems(section).length) }
                : undefined
            "
          >
            <header v-if="category === 'all' && !bestMatch(section)">
              <h2>
                <button
                  v-if="section.category"
                  class="search-section-link"
                  :aria-label="`查看全部${section.title}`"
                  @click="choose(section.category)"
                >
                  {{ section.title }}<Icon name="forward" /></button
                ><template v-else>{{ section.title }}</template>
              </h2>
              <button
                v-if="!guestReady && section.items.some((i) => i.kind === 'song')"
                class="icon-button search-section-play"
                aria-label="播放全部搜索单曲"
                :disabled="
                  !playable || !comprehensivePlayAllTracks(section).some((t) => !t.missing)
                "
                @click="playComprehensiveAll(section)"
              >
                <Icon name="play" />
              </button>
            </header>
            <SearchCardGrid
              v-if="guestReady"
              :items="section.items"
              :scroller="panel"
              :virtual="waterfall"
              v-slot="{ item }"
            >
              <GuestSearchResult
                :item="item"
                :keyword="matchKeyword"
                @search="emit('search', $event)"
              />
            </SearchCardGrid>
            <div
              v-else-if="category === 'lyrics'"
              class="search-lyric-list"
              role="table"
              aria-label="歌词搜索结果"
              :aria-rowcount="lyricItems.length * 2 + 1"
            >
              <div class="track-columns track-header" role="row">
                <span>#</span><span role="columnheader">歌曲 / 歌手</span
                ><span role="columnheader">专辑</span><span role="columnheader">喜欢</span
                ><span role="columnheader">时长</span>
              </div>
              <div
                :ref="(element) => (lyricBody = (element as HTMLElement | null) ?? undefined)"
                class="search-lyric-window"
                role="presentation"
                :style="{ height: `${lyricWindow.height.value}px` }"
                @focusin="lyricFocus"
                @focusout="lyricBlur"
              >
                <div
                  v-for="{ item, index, key } in renderedLyrics"
                  :key="key"
                  class="search-lyric-result"
                  :class="{ 'lyric-striped': index % 2 }"
                  :data-lyric-index="index"
                  :style="{ top: `${lyricWindow.offsets.value[index]}px` }"
                  role="rowgroup"
                  :data-result-key="`lyric:${key}`"
                >
                  <TrackRow
                    :track="item.track"
                    :match-keyword="matchKeyword"
                    :index="index"
                    :aria-rowindex="index * 2 + 2"
                    :liked="liked.has(item.track.id)"
                    :busy="busyLikes.has(item.track.id)"
                    :current-id="currentId"
                    :current-toggle="
                      !item.track.missing &&
                      !item.track.local &&
                      currentToggle?.trackId === item.track.id
                        ? currentToggle
                        : undefined
                    "
                    :pending-toggle="
                      !item.track.missing &&
                      !item.track.local &&
                      pendingTrackToggle?.trackId === item.track.id
                        ? pendingTrackToggle
                        : undefined
                    "
                    :playable="playable"
                    navigable
                    @play="(_index, gesture) => playTrack(item.track, gesture === 'double_click')"
                    @toggle-current="(target, anchor) => emit('toggleCurrent', target, anchor)"
                    @toggle-pending="(target, anchor) => emit('togglePendingTrack', target, anchor)"
                    @like="emit('like', $event)"
                    @more="showTrackMenu"
                    @artist="emit('artist', $event)"
                    @album="emit('album', $event)"
                  />
                  <div role="row" :aria-rowindex="index * 2 + 3">
                    <div role="cell" aria-colspan="5">
                      <SearchLyricExtra
                        :lines="item.excerpt"
                        :truncated="item.excerptTruncated"
                        :content-id="`search-lyric-${index}`"
                        :expanded="expandedLyrics.has(key)"
                        @toggle="toggleLyric(key)"
                      />
                    </div>
                  </div>
                </div>
              </div>
            </div>
            <div
              v-else-if="category === 'all' && section.items.some((i) => i.kind === 'song')"
              class="search-track-preview"
            >
              <SearchTrackCard
                v-for="(item, index) in section.items
                  .filter((item) => item.kind === 'song')
                  .slice(0, 6)"
                :key="`${item.track.id}:${index}`"
                :track="item.track"
                :mv-id="item.mvId"
                :replacement-mv="item.replacementMv"
                :vip-badge="item.vipBadge"
                :limited-free="item.limitedFree"
                :quality-badge="item.qualityBadge"
                :hint="item.hint"
                :explicit="item.explicit"
                :original-recording="item.originalRecording"
                :reasons="item.reasons"
                @reason="(label, target) => emit('reason', label, target)"
                :match-keyword="matchKeyword"
                :playable="playable"
                :current="item.track.id === currentId"
                :playing="playbackState === 'playing' || playbackState === 'buffering'"
                :pending-toggle="
                  pendingTrackToggle?.trackId === item.track.id ? pendingTrackToggle : undefined
                "
                :liked="liked.has(item.track.id)"
                :busy-like="busyLikes.has(item.track.id)"
                @play="playTrack(item.track, true)"
                @pending-toggle="emit('togglePendingTrack', $event)"
                @like="emit('like', item.track)"
                @more="
                  (event) =>
                    showTrackMenu({
                      track: item.track,
                      event,
                      play: () => playTrack(item.track),
                    })
                "
                @download="emit('downloadTrack', item.track)"
                @mv="emit('mv', $event)"
                @artist="emit('artist', $event)"
              />
            </div>
            <TrackTable
              v-else-if="section.items.some((i) => i.kind === 'song')"
              ref="searchSongTables"
              :tracks="tracks(section)"
              :search-title-hints="category === 'songs' ? songTableHints(section) : undefined"
              :queue-drop-enabled="category === 'songs' && queueDropEnabled"
              :selected-menu-enabled="category === 'songs' && playable"
              :selection-label="category === 'songs' ? '单曲搜索结果' : undefined"
              :playlist-selection-scope="
                category === 'songs'
                  ? JSON.stringify([account.epoch, searchId, keyword, offset, section.id])
                  : undefined
              "
              :original-recordings="originalRecordings"
              :match-keyword="matchKeyword"
              :liked="liked"
              :busy="busyLikes"
              :playable="playable"
              :current-id="currentId"
              :current-toggle="currentToggle"
              :pending-toggle="pendingTrackToggle"
              navigable
              @queue-drag="(event, rows, current) => startQueueDrop(event, section, rows, current)"
              @play="
                (index, gesture) => playTrack(tracks(section)[index], gesture === 'double_click')
              "
              @toggle-current="(target, anchor) => emit('toggleCurrent', target, anchor)"
              @toggle-pending="(target, anchor) => emit('togglePendingTrack', target, anchor)"
              @like="emit('like', $event)"
              @more="showTrackMenu"
              @selected-more="
                (request, anchor, rows, current) =>
                  showSelectedTrackMenu(section, request, anchor, rows, current)
              "
              @artist="emit('artist', $event)"
              @album="emit('album', $event)"
            />
            <SearchCardGrid
              v-if="!guestReady && section.items.some((i) => i.kind !== 'song')"
              ref="searchCardGrids"
              :items="previewItems(section)"
              :scroller="panel"
              :virtual="gridWaterfall"
              :preview="oneRowPreview(section)"
              :compact-preview="compactPreview && oneRowPreview(section)"
              :carousel="carousel(section)"
              :table="tableKind"
              @position="previewPosition(section, $event)"
              v-slot="{ item, index }"
            >
              <SearchResultCard
                :item="item"
                :category="category"
                :table-row="tableKind ? { index, number: offset + index + 1 } : undefined"
                :match-keyword="matchKeyword"
                :playable="playable"
                :queueable="queueable"
                :podcast-next-enabled="
                  category === 'podcasts' &&
                  voiceQueueable &&
                  playbackState !== 'preparing' &&
                  playbackState !== 'loading'
                "
                :voice-next="
                  category === 'voices' || (category === 'all' && section.category === 'voices')
                    ? voiceQueueable
                      ? 'ready'
                      : 'disabled'
                    : undefined
                "
                :voice-links="
                  account.status === 'authenticated' && searchVoiceLinkSection(category, section)
                "
                :voice-menu-epoch="
                  account.status === 'authenticated' && searchVoiceLinkSection(category, section)
                    ? account.epoch
                    : undefined
                "
                :playlist-action-busy="playlistActionBusy"
                :playlist-known="
                  item.kind === 'playlist' ? knownPlaylistIds?.has(item.playlist.id) : undefined
                "
                :album-subscribed="
                  item.kind === 'album' ? observedAlbumMembership.get(item.album.id) : undefined
                "
                :podcast-follow="
                  item.kind === 'podcast' && item.table?.subscriptionEligible
                    ? podcastFollow.control(item.podcast.id)
                    : undefined
                "
                :current="searchCardIsCurrent(item, playbackOrigin, currentVoiceId)"
                :pending-voice-toggle="
                  item.kind === 'voice' && item.voice.id === pendingVoiceToggle?.voiceId
                    ? pendingVoiceToggle
                    : undefined
                "
                :playing="playbackState === 'playing' || playbackState === 'buffering'"
                @open="open"
                @creator="emit('creator', $event)"
                @artist="emit('artist', $event)"
                @artist-action="emit('artistAction', $event)"
                @album-action="emit('albumAction', $event)"
                @podcast-follow="
                  item.kind === 'podcast' &&
                  item.table?.subscriptionEligible &&
                  podcastFollow.activate(item.podcast.id, $event)
                "
                @play="(item, gesture) => emit('playCard', item, gesture)"
                @toggle-pending-voice="
                  (target, anchor) => emit('togglePendingVoice', target, anchor)
                "
                @queue="emit('queueCard', $event)"
                @queue-voice="emit('queueVoice', $event)"
                @voice-link="activateVoiceLink(section, $event)"
                @voice-menu-error="voiceLinkStatus = $event"
                @voice-menu-play="emit('voiceMenuPlay', $event)"
                @playlist-download="emit('playlistDownload', $event)"
                @playlist-link="(list, action) => emit('playlistLink', list, action)"
                @playlist-subscribe="emit('playlistSubscribe', $event)"
                @comments="emit('cardComments', $event)"
                @download="emit('cardDownload', $event)"
              />
            </SearchCardGrid>
          </section>
          <nav
            v-if="!waterfall && category !== 'all' && (offset > 0 || view.more)"
            class="search-result-pagination"
            aria-label="搜索结果分页"
          >
            <button
              class="secondary"
              :disabled="offset === 0"
              @click="turn(category, pagination?.previous)"
            >
              上一页</button
            ><span>第 {{ pagination?.number ?? 1 }} 页</span
            ><button
              class="secondary"
              :disabled="!nextPage"
              @click="nextPage && turn(category, nextPage)"
            >
              下一页
            </button>
          </nav>
          <div
            v-if="waterfall && (count || view.more)"
            ref="moreFooter"
            class="search-load-more"
            :aria-busy="loadingMore"
            tabindex="-1"
          >
            <span v-if="moreError" role="alert">{{ moreError }}</span>
            <button
              v-if="view.more"
              ref="moreButton"
              class="text-button"
              :aria-disabled="loadingMore"
              @click="model.more()"
            >
              {{ loadingMore ? '正在加载更多…' : moreError ? '重试加载更多' : '加载更多' }}
            </button>
            <span v-else>没有更多可显示的结果</span>
            <button
              v-if="
                playlistWaterfall ||
                (songWaterfall && (moreError || count >= SEARCH_PLAYBACK_ROW_LIMIT))
              "
              class="text-button"
              @click="turn(category, firstNumberedPage)"
            >
              使用分页查看
            </button>
          </div>
        </template>
      </div>
    </Transition>
    <ModalTransition
      :return-focus="() => voiceLinkAnchor"
      :restore-focus="voiceLinkRestoreFocus"
      :fallback-focus="() => tabs?.querySelector<HTMLElement>('[aria-selected=true]') ?? null"
    >
      <TrackShareDialog
        v-if="voiceLinkShare"
        :program="voiceLinkShare.program"
        :can-copy="voiceLinkShare.canCopy"
        @close="clearVoiceLink(voiceLinkShare.canCopy())"
      />
    </ModalTransition>
  </section>
</template>
