import { computed, ref, shallowRef } from 'vue';
import { useCore } from '../services/core.ts';
import {
  failure,
  type AccountView,
  type CoreClient,
  type TrackPage,
} from '../services/contracts.ts';
import { defaultLocalQuery, type LocalPage, type LocalTrack } from '../services/local.ts';
import type { LocalMatchView, MatchCommand } from '../services/localMatching.ts';

export function useLocalMatching(
  core: Pick<CoreClient, 'localMatchView' | 'localMatchCommand' | 'searchSongs'> = useCore(),
  sessionChanged = () => {},
) {
  const view = shallowRef<LocalMatchView>();
  const ready = ref(false),
    busy = ref(false),
    error = ref(''),
    searching = ref(false),
    searchError = ref('');
  const target = shallowRef<LocalTrack>();
  const results = shallowRef<TrackPage>();
  const keyword = ref('');
  let epoch = '',
    searchedKeyword = '',
    generation = 0,
    read = 0,
    search = 0,
    disposed = false,
    page: LocalPage | undefined;
  const running = computed(() => ['queued', 'running'].includes(view.value?.job?.phase ?? ''));
  const unfinished = computed(
    () => !!view.value?.job && !['complete', 'cancelled'].includes(view.value.job.phase),
  );
  function issue(value: unknown, searching = false) {
    const problem = failure(value);
    (searching ? searchError : error).value = problem.message;
    if (['session_expired', 'profile_changed', 'stale'].includes(problem.code)) sessionChanged();
  }
  function connect(account: AccountView) {
    generation++;
    read++;
    search++;
    epoch = account.epoch;
    ready.value = account.status === 'authenticated';
    busy.value = false;
    searching.value = false;
    error.value = '';
    view.value = undefined;
    close();
    if (ready.value && page) void refresh();
  }
  function close() {
    target.value = undefined;
    results.value = undefined;
    searchError.value = '';
    search++;
    searching.value = false;
  }
  function setPage(value: LocalPage | undefined) {
    page = value;
    if (value && ready.value) void refresh();
    else {
      read++;
      view.value = undefined;
    }
  }
  async function refresh() {
    if (disposed || !ready.value || !page) return;
    const gen = generation,
      serial = ++read,
      captured = epoch;
    try {
      const response = await core.localMatchView(
        captured,
        page.tracks.map((r) => r.track.id),
      );
      if (disposed || gen !== generation || serial !== read || response.epoch !== captured) return;
      view.value = response.value;
      error.value = '';
    } catch (e) {
      if (!disposed && gen === generation && serial === read) issue(e);
    }
  }
  async function command(input: MatchCommand) {
    if (disposed || !ready.value || busy.value) return false;
    const gen = generation;
    busy.value = true;
    error.value = '';
    read++;
    try {
      await core.localMatchCommand(epoch, input);
      if (disposed || gen !== generation) return false;
      await refresh();
      return true;
    } catch (e) {
      if (!disposed && gen === generation) issue(e);
      return false;
    } finally {
      if (!disposed && gen === generation) busy.value = false;
    }
  }
  function start() {
    return command({ kind: 'start', query: defaultLocalQuery() });
  }
  function control(action: 'pause' | 'resume' | 'cancel') {
    const job = view.value?.job;
    if (job) return command({ kind: 'control', id: job.id, action });
  }
  function link(row: LocalTrack) {
    return view.value?.links.find((l) => l.id === row.track.id && l.revision === row.revision);
  }
  function open(row: LocalTrack) {
    if (!ready.value || running.value || busy.value || row.track.missing) return;
    close();
    target.value = row;
    keyword.value = `${row.track.name} ${row.track.artists[0] ?? ''}`.trim().slice(0, 256);
  }
  async function find(offset = 0) {
    if (!target.value || !ready.value || disposed || !keyword.value.trim()) return;
    const gen = generation,
      serial = ++search,
      captured = epoch;
    searching.value = true;
    searchError.value = '';
    if (searchedKeyword !== keyword.value.trim()) offset = 0;
    searchedKeyword = keyword.value.trim();
    try {
      const response = await core.searchSongs(keyword.value.trim(), { offset, limit: 20 });
      if (disposed || gen !== generation || serial !== search || response.epoch !== captured)
        return;
      results.value = response.value;
    } catch (e) {
      if (!disposed && gen === generation && serial === search) issue(e, true);
    } finally {
      if (!disposed && gen === generation && serial === search) searching.value = false;
    }
  }
  async function bind(songId: string) {
    const row = target.value;
    const gen = generation;
    if (
      row &&
      (await command({
        kind: 'bind',
        id: row.track.id,
        revision: row.revision,
        song_id: songId,
      })) &&
      gen === generation
    )
      close();
  }
  async function clear() {
    const row = target.value;
    const gen = generation;
    if (
      row &&
      (await command({ kind: 'clear', id: row.track.id, revision: row.revision })) &&
      gen === generation
    )
      close();
  }
  function dispose() {
    disposed = true;
    generation++;
    read++;
    close();
  }
  return {
    view,
    ready,
    busy,
    error,
    running,
    unfinished,
    target,
    results,
    keyword,
    searching,
    searchError,
    connect,
    setPage,
    refresh,
    command,
    start,
    control,
    link,
    open,
    close,
    find,
    bind,
    clear,
    dispose,
  };
}
export type LocalMatchingModel = ReturnType<typeof useLocalMatching>;
