import { ref, shallowRef } from 'vue';
import type { AccountView } from '../services/contracts.ts';
import { failure } from '../services/contracts.ts';
import { searchCardKey } from '../services/searchCards.ts';
import {
  createSearchViewportMemory,
  type SearchPosition,
  type SearchViewportMemory,
} from '../services/searchViewport.ts';
import type {
  SearchCategory,
  SearchResultQuery,
  SearchResultView,
  SearchResultsClient,
} from '../services/search.ts';

// Keep a song/lyric visit within the native playback selection and renderer limits.
export const SEARCH_PLAYBACK_ROW_LIMIT = 500;
const LYRIC_TEXT_LIMIT = 2 * 1024 * 1024;
const lyricSize = (item: SearchResultView['sections'][number]['items'][number]) =>
  item.kind === 'song' ? item.excerpt.reduce((size, line) => size + line.length + 1, 0) : 0;

export function useSearchResults(
  core: SearchResultsClient,
  expired: () => void,
  memory: SearchViewportMemory = createSearchViewportMemory(),
) {
  const view = shallowRef<SearchResultView>(),
    tabs = shallowRef<readonly SearchCategory[]>(),
    loading = ref(false),
    error = ref(''),
    loadingMore = ref(false),
    moreError = ref(''),
    manualMore = ref(false),
    position = shallowRef<SearchPosition>({ top: 0, tracks: 0 });
  let activeKey = '';
  let generation = 0,
    account: AccountView | undefined;
  function connect(next: AccountView) {
    generation++;
    account = next;
    memory.connect(next);
    view.value = undefined;
    tabs.value = undefined;
    error.value = '';
    loading.value = false;
    loadingMore.value = false;
    moreError.value = '';
    manualMore.value = false;
  }
  function savePosition(next: SearchPosition) {
    position.value = next;
    save();
  }
  function save() {
    if (view.value)
      memory.set(
        activeKey,
        {
          view: view.value,
          position: position.value,
          moreError: moreError.value,
          manualMore: manualMore.value,
        },
        account?.epoch ?? '',
      );
  }
  async function open(query: SearchResultQuery, reuse = false, session = 0) {
    if (account?.status !== 'authenticated') return;
    const epoch = account.epoch,
      current = ++generation;
    activeKey = memory.key(query, session);
    const cached = reuse ? memory.get(activeKey, epoch) : undefined;
    tabs.value = memory.getTabs(session, epoch);
    loadingMore.value = false;
    moreError.value = cached?.moreError ?? '';
    manualMore.value = cached?.manualMore ?? false;
    position.value = cached?.position ?? { top: 0, tracks: 0 };
    if (cached) {
      view.value = cached.view;
      if (!tabs.value && query.category === 'all') tabs.value = cached.view.tabs;
      error.value = '';
      loading.value = false;
      return;
    }
    view.value = undefined;
    loading.value = true;
    error.value = '';
    try {
      const result = await core.searchResults(epoch, query);
      if (current === generation && account?.epoch === epoch && result.epoch === epoch) {
        view.value = result.value;
        if (
          query.category === 'all' &&
          result.value.query.category === 'all' &&
          result.value.tabs
        ) {
          memory.setTabs(session, result.value.tabs, epoch);
          tabs.value = result.value.tabs;
        }
        save();
      }
    } catch (e) {
      if (current !== generation) return;
      const issue = failure(e);
      error.value = issue.message;
      if (issue.code === 'session_expired' || issue.code === 'profile_changed') {
        memory.clear();
        tabs.value = undefined;
        expired();
      }
    } finally {
      if (current === generation) loading.value = false;
    }
  }
  async function more() {
    const previous = view.value;
    if (
      !previous?.more ||
      loading.value ||
      loadingMore.value ||
      account?.status !== 'authenticated'
    )
      return;
    const current = generation,
      epoch = account.epoch;
    loadingMore.value = true;
    moreError.value = '';
    try {
      if (previous.nextOffset <= previous.query.offset || previous.nextOffset > 10000)
        throw { code: 'unavailable', message: '暂时无法继续加载，请重新搜索' };
      const result = await core.searchResults(epoch, {
        ...previous.query,
        keyword:
          (previous.query.correct ? previous.correction : undefined) ?? previous.query.keyword,
        offset: previous.nextOffset,
      });
      if (current !== generation || account?.epoch !== epoch || result.epoch !== epoch) return;
      const next = result.value;
      if (next.nextOffset <= previous.nextOffset && next.more)
        throw { code: 'unavailable', message: '搜索结果未更新，请重试' };
      let added = 0;
      const lyrics = previous.query.category === 'lyrics';
      const songs = previous.query.category === 'songs';
      let songRows = songs
        ? previous.sections.reduce((sum, section) => sum + section.items.length, 0)
        : 0;
      const songKeys = new Set(
        songs ? previous.sections.flatMap((section) => section.items.map(searchCardKey)) : [],
      );
      let lyricRows = 0,
        lyricUnits = 0,
        capped = false;
      if (lyrics)
        for (const section of previous.sections)
          for (const item of section.items) {
            lyricRows++;
            lyricUnits += lyricSize(item);
          }
      // The native cursor counts raw rows, including ones it cannot safely expose.
      // An entirely filtered page is not the same as a repeated visible page.
      const filteredPage =
        next.unavailableCount > 0 && next.sections.every((section) => !section.items.length);
      const sections = previous.sections.map((s) => ({ ...s, items: [...s.items] }));
      for (const incoming of next.sections) {
        let section = sections.find((s) => s.id === incoming.id);
        if (!section) {
          section = { ...incoming, items: [] };
          sections.push(section);
        }
        const seen = new Set(section.items.map(searchCardKey));
        for (const item of incoming.items) {
          const key = searchCardKey(item);
          if (lyrics) {
            const size = lyricSize(item);
            if (lyricRows >= SEARCH_PLAYBACK_ROW_LIMIT || lyricUnits + size > LYRIC_TEXT_LIMIT) {
              capped = true;
              break;
            }
            // Two lyric hits may refer to the same song but have different excerpts.
            lyricRows++;
            lyricUnits += size;
            section.items.push(item);
            added++;
          } else if (songs) {
            if (songKeys.has(key)) continue;
            if (songRows >= SEARCH_PLAYBACK_ROW_LIMIT) {
              capped = true;
              break;
            }
            songKeys.add(key);
            songRows++;
            section.items.push(item);
            added++;
          } else if (!seen.has(key)) {
            section.items.push(item);
            seen.add(key);
            added++;
          }
        }
        if (capped) break;
      }
      capped ||=
        next.more &&
        (lyrics
          ? lyricRows === SEARCH_PLAYBACK_ROW_LIMIT
          : songs && songRows === SEARCH_PLAYBACK_ROW_LIMIT);
      const more = next.more && !capped;
      // The server cursor can advance past duplicate or unrenderable rows.
      // Require a click when nothing was added to avoid a scroll-triggered loop.
      manualMore.value = more && added === 0;
      view.value = {
        ...next,
        query: previous.query,
        correction: previous.query.correct ? (next.correction ?? previous.correction) : null,
        sections,
        more,
        unavailableCount: previous.unavailableCount + next.unavailableCount,
        notice: capped
          ? lyrics
            ? '歌词结果已达到本次浏览上限，可更换关键词继续查找。'
            : '单曲结果已达到本次连续浏览上限，可使用分页查看或更换关键词。'
          : manualMore.value
            ? filteredPage
              ? '这一批结果暂无法展示，可以继续加载。'
              : '这一批没有新的结果，可以继续加载。'
            : next.notice,
      };
      save();
    } catch (e) {
      if (current !== generation) return;
      const issue = failure(e);
      moreError.value = issue.message;
      save();
      if (issue.code === 'session_expired' || issue.code === 'profile_changed') {
        memory.clear();
        tabs.value = undefined;
        expired();
      }
    } finally {
      if (current === generation) loadingMore.value = false;
    }
  }
  function dispose() {
    generation++;
    account = undefined;
    view.value = undefined;
    tabs.value = undefined;
    loading.value = false;
    loadingMore.value = false;
    manualMore.value = false;
  }
  return {
    view,
    tabs,
    loading,
    error,
    loadingMore,
    moreError,
    manualMore,
    position,
    connect,
    open,
    more,
    savePosition,
    dispose,
  };
}
