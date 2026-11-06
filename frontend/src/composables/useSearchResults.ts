import { ref, shallowRef } from 'vue';
import type { AccountView } from '../services/contracts.ts';
import { failure } from '../services/contracts.ts';
import { searchCardKey } from '../services/searchCards.ts';
import {
  createSearchViewportMemory,
  withoutSearchFooters,
  type SearchCorrectionReminder,
  type SearchPosition,
  type SearchViewportMemory,
} from '../services/searchViewport.ts';
import type { PcPresentationView } from '../services/pcPresentation.ts';
import { patchSearchPresentation } from '../services/searchPresentation.ts';
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
    correctionReminder = shallowRef<SearchCorrectionReminder>(),
    loading = ref(false),
    error = ref(''),
    loadingMore = ref(false),
    moreError = ref(''),
    manualMore = ref(false),
    position = shallowRef<SearchPosition>({ top: 0, tracks: 0 });
  const presentationChange = ref(0);
  let activeKey = '';
  let generation = 0,
    account: AccountView | undefined;
  let privilegeSerial = 0;
  let presentationSerial = 0;
  let observed: string | undefined;
  let closed = false;
  let guest = false;
  const canRead = () => account?.status === 'authenticated' || guest;
  let unlistenPrivileges: (() => void) | undefined;
  const unlistenPresentation = memory.onPresentation((next) => {
    if (closed || next.epoch !== account?.epoch || account.status !== 'authenticated') return;
    const current = view.value;
    if (current?.query.category !== 'all') return;
    if (!next.searchFooter) replacePresentation(withoutSearchFooters(current));
    void refreshPresentation(next);
  });
  function replacePresentation(next: SearchResultView) {
    if (view.value === next) return;
    view.value = next;
    presentationChange.value++;
    save();
  }
  async function projectPresentation(
    source: SearchResultView,
    epoch: string,
    observation: PcPresentationView | undefined,
    restoringCache = false,
  ): Promise<SearchResultView> {
    if (source.query.category !== 'all') return source;
    if (!observation) return restoringCache ? withoutSearchFooters(source) : source;
    const safe = withoutSearchFooters(source);
    if (!source.presentationReceipt || !core.searchPresentation) return safe;
    if (!restoringCache && source.presentationRevision === observation.revision)
      return observation.searchFooter ? source : safe;
    try {
      const reply = await core.searchPresentation(
        epoch,
        source.presentationReceipt,
        observation.revision,
      );
      if (reply.epoch !== epoch || reply.value.revision !== observation.revision) return safe;
      const projected = patchSearchPresentation(source, reply.value) ?? safe;
      return observation.searchFooter ? projected : withoutSearchFooters(projected);
    } catch {
      // Missing/evicted receipts retain ordinary results, never replay a search.
      return safe;
    }
  }
  async function refreshPresentation(observation: PcPresentationView) {
    const current = view.value;
    const epoch = account?.status === 'authenticated' ? account.epoch : undefined;
    if (!current || !epoch || current.query.category !== 'all') return;
    const visit = generation,
      serial = ++presentationSerial;
    const next = await projectPresentation(current, epoch, observation);
    if (
      closed ||
      generation !== visit ||
      serial !== presentationSerial ||
      account?.epoch !== epoch ||
      memory.presentation(epoch)?.revision !== observation.revision ||
      view.value?.presentationReceipt !== current.presentationReceipt
    )
      return;
    // Membership may have updated while projection was pending. Merge into its
    // newest rows rather than restoring an older privilege/play-all snapshot.
    const latest = view.value;
    if (!latest) return;
    const patch =
      next.presentationRevision === observation.revision
        ? {
            receipt: next.presentationReceipt!,
            revision: next.presentationRevision,
            previewLayout: next.previewLayout ?? null,
            sections: next.sections.flatMap((section) => {
              const items = section.items
                .flatMap((item, index) =>
                  item.kind === 'song'
                    ? [
                        {
                          index,
                          trackId: item.track.id,
                          hint: item.hint ?? null,
                          reasons: item.reasons ?? [],
                        },
                      ]
                    : [],
                )
                .slice(0, 6);
              return items.length ? [{ id: section.id, items }] : [];
            }),
          }
        : undefined;
    replacePresentation(
      patch
        ? (patchSearchPresentation(latest, patch) ?? withoutSearchFooters(latest))
        : withoutSearchFooters(latest),
    );
  }
  if (core.onSearchPrivilegeChanged) {
    void core
      .onSearchPrivilegeChanged(({ epoch, revision }) => {
        if (account?.epoch !== epoch || observed === revision) return;
        observed = revision;
        void refreshPrivileges(revision);
      })
      .then((unlisten) => {
        if (closed) unlisten();
        else unlistenPrivileges = unlisten;
      })
      .catch(() => {});
  }
  async function refreshPrivileges(observation?: string) {
    const current = view.value;
    const epoch = account?.status === 'authenticated' ? account.epoch : undefined;
    const receipt = current?.privilegeReceipt;
    if (!epoch || !receipt || current.query.category !== 'all' || !core.searchRefreshPrivileges)
      return;
    const visit = generation;
    const serial = ++privilegeSerial;
    const active = () =>
      account?.epoch === epoch &&
      visit === generation &&
      serial === privilegeSerial &&
      current.privilegeReceipt === view.value?.privilegeReceipt &&
      !closed;
    try {
      const reply = await core.searchRefreshPrivileges(epoch, receipt, observation);
      if (!active() || reply.epoch !== epoch || reply.value.receipt !== receipt) return;
      const { section, rows, playAllIds } = reply.value;
      if (
        !/^search_block_song:[0-9]+$/.test(section) ||
        !playAllIds.length ||
        playAllIds.length > 6
      )
        return;
      const patches = new Map(rows.map((row) => [row.id, row]));
      const latest = view.value;
      if (!latest) return;
      const sections = latest.sections.map((block) => {
        if (block.id !== section || block.category !== 'songs') return block;
        let songs = 0;
        const shown = block.items
          .flatMap((item) => (item.kind === 'song' ? [item.track.id] : []))
          .slice(0, 6);
        if (
          shown.length !== playAllIds.length ||
          shown.some((id, index) => id !== playAllIds[index])
        )
          return block;
        return {
          ...block,
          playAllIds,
          items: block.items.map((item) => {
            if (item.kind !== 'song' || songs++ >= 6) return item;
            const row = patches.get(item.track.id);
            if (!row) return item;
            return {
              ...item,
              track: { ...item.track, coverUrl: row.coverUrl },
              mvId: row.mvId ?? undefined,
              replacementMv: row.replacementMv,
              vipBadge: row.vipBadge,
              limitedFree: row.limitedFree,
              qualityBadge: row.qualityBadge ?? undefined,
            };
          }),
        };
      });
      view.value = { ...latest, sections };
      save();
    } catch (e) {
      if (active() && ['session_expired', 'profile_changed'].includes(failure(e).code)) expired();
      // A refresh error does not replace current content or trigger a retry.
    }
  }
  function connect(next: AccountView, guestReady = false) {
    generation++;
    if (next.epoch !== account?.epoch || next.status !== account?.status) observed = undefined;
    account = next;
    guest = guestReady && (next.status === 'unknown' || next.status === 'signed_out');
    memory.connect(next);
    view.value = undefined;
    tabs.value = undefined;
    correctionReminder.value = undefined;
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
  async function open(
    query: SearchResultQuery,
    reuse = false,
    session = 0,
    originalKeyword = query.keyword,
    defaultReceipt?: string,
  ) {
    if (!account || !canRead()) return;
    const epoch = account.epoch,
      current = ++generation;
    activeKey = memory.key(query, session);
    const cached = reuse ? memory.get(activeKey, epoch) : undefined;
    tabs.value = memory.getTabs(session, epoch);
    const updateReminder = (view?: SearchResultView) => {
      const reminder =
        query.correct &&
        view?.query.category === 'all' &&
        view.correction &&
        view.correctionParts?.length
          ? { rewrite: view.correction, parts: view.correctionParts }
          : undefined;
      correctionReminder.value = reminder;
      memory.setReminder(session, originalKeyword, epoch, reminder);
    };
    if (query.category === 'all') updateReminder(cached?.view);
    else
      correctionReminder.value = query.correct
        ? memory.getReminder(session, originalKeyword, epoch)
        : undefined;
    loadingMore.value = false;
    moreError.value = cached?.moreError ?? '';
    manualMore.value = cached?.manualMore ?? false;
    position.value = cached?.position ?? { top: 0, tracks: 0 };
    if (cached) {
      view.value = undefined;
      loading.value = true;
      const observation = memory.presentation(epoch);
      const projected = await projectPresentation(cached.view, epoch, observation, true);
      if (closed || current !== generation || account?.epoch !== epoch) return;
      const latest = memory.presentation(epoch);
      view.value =
        latest?.revision !== observation?.revision ? withoutSearchFooters(projected) : projected;
      if (!tabs.value && query.category === 'all') tabs.value = cached.view.tabs;
      error.value = '';
      loading.value = false;
      save();
      if (latest && latest.revision !== observation?.revision) void refreshPresentation(latest);
      // The official mount effect has something to update only if the
      // model already holds rows. Never add a second read to a cold search.
      if (query.category === 'all') void refreshPrivileges();
      return;
    }
    view.value = undefined;
    loading.value = true;
    error.value = '';
    try {
      const result =
        query.category === 'all' && query.channel === 'defaultquery' && defaultReceipt
          ? await core.searchResults(epoch, query, defaultReceipt)
          : await core.searchResults(epoch, query);
      if (current === generation && account?.epoch === epoch && result.epoch === epoch) {
        const observation = memory.presentation(epoch);
        const projected = await projectPresentation(result.value, epoch, observation);
        if (closed || current !== generation || account?.epoch !== epoch) return;
        const latest = memory.presentation(epoch);
        view.value =
          latest?.revision !== observation?.revision ? withoutSearchFooters(projected) : projected;
        if (query.category === 'all') updateReminder(result.value);
        if (
          query.category === 'all' &&
          result.value.query.category === 'all' &&
          result.value.tabs
        ) {
          memory.setTabs(session, result.value.tabs, epoch);
          tabs.value = result.value.tabs;
        }
        save();
        if (latest && latest.revision !== observation?.revision) void refreshPresentation(latest);
      }
    } catch (e) {
      if (current !== generation) return;
      const issue = failure(e);
      error.value = issue.message;
      if (!guest && (issue.code === 'session_expired' || issue.code === 'profile_changed')) {
        memory.clear();
        tabs.value = undefined;
        correctionReminder.value = undefined;
        expired();
      }
    } finally {
      if (current === generation) loading.value = false;
    }
  }
  async function more() {
    const previous = view.value;
    if (!previous?.more || loading.value || loadingMore.value || !account || !canRead()) return;
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
      if (!guest && (issue.code === 'session_expired' || issue.code === 'profile_changed')) {
        memory.clear();
        tabs.value = undefined;
        correctionReminder.value = undefined;
        expired();
      }
    } finally {
      if (current === generation) loadingMore.value = false;
    }
  }
  function dispose() {
    generation++;
    closed = true;
    unlistenPrivileges?.();
    unlistenPrivileges = undefined;
    unlistenPresentation();
    account = undefined;
    view.value = undefined;
    tabs.value = undefined;
    correctionReminder.value = undefined;
    loading.value = false;
    loadingMore.value = false;
    manualMore.value = false;
  }
  return {
    view,
    tabs,
    correctionReminder,
    loading,
    error,
    loadingMore,
    moreError,
    manualMore,
    position,
    presentationChange,
    connect,
    open,
    more,
    savePosition,
    dispose,
  };
}
