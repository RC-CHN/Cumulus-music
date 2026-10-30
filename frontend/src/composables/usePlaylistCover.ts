import { ref, shallowRef } from 'vue';
import { failure, type AccountView, type CoreClient } from '../services/contracts.ts';
import { useCore } from '../services/core.ts';
import {
  coverEditable,
  coverTerminal,
  type CoverTask,
  type CoverImage,
  type CoverCommand,
  type CoverOutcome,
} from '../services/playlistCover.ts';
import type { PlaylistEditor } from '../services/playlists.ts';

export function usePlaylistCover(
  core: CoreClient = useCore(),
  saved: (outcome: CoverOutcome) => Promise<void> = async () => {},
  expired: () => void = () => {},
) {
  const task = shallowRef<CoverTask>(),
    image = shallowRef<CoverImage>(),
    error = ref(''),
    warning = ref('');
  const busy = ref(false),
    loading = ref(false),
    readFailed = ref(false),
    stopping = ref(false),
    changedElsewhere = ref(false),
    latest = shallowRef<PlaylistEditor>();
  let epoch = '',
    playlist = '',
    serial = 0,
    read = 0,
    connected = false,
    disposed = false;
  let poll: ReturnType<typeof setTimeout> | undefined;
  const announced = new Set<string>();
  const live = (e: string, n: number) => connected && !disposed && epoch === e && serial === n;
  function stopPoll() {
    if (poll) clearTimeout(poll);
    poll = undefined;
  }
  function problem(value: unknown) {
    const p = failure(value);
    if (p.code !== 'stale') error.value = p.message;
    if (p.code === 'snapshot_changed') changedElsewhere.value = true;
    if (['session_expired', 'profile_changed'].includes(p.code)) {
      connected = false;
      serial++;
      read++;
      stopPoll();
      task.value = undefined;
      image.value = undefined;
      busy.value = false;
      loading.value = false;
      expired();
    }
  }
  async function refresh(e = epoch, n = serial) {
    if (!live(e, n)) return;
    const turn = ++read;
    try {
      const result = await core.playlistCoverTasks(e);
      if (!live(e, n) || result.epoch !== e || turn !== read) return;
      readFailed.value = false;
      const rows = result.value.filter((t) => t.playlistId === playlist);
      const selected = task.value ? rows.find((t) => t.id === task.value?.id) : undefined;
      task.value =
        selected ?? rows.find((t) => !coverTerminal(t)) ?? rows.find((t) => t.cleanupPending);
      if (task.value?.error === 'playlist_changed') changedElsewhere.value = true;
    } catch (value) {
      if (live(e, n) && turn === read) {
        readFailed.value = true;
        problem(value);
      }
    }
  }
  async function preview(e = epoch, n = serial) {
    const id = task.value?.id;
    if (!id || !task.value || coverTerminal(task.value)) return;
    try {
      const result = await core.playlistCoverImage(e, id);
      if (live(e, n) && result.epoch === e && task.value?.id === id) image.value = result.value;
    } catch (value) {
      if (live(e, n) && task.value?.id === id) problem(value);
    }
  }
  async function connect(account: AccountView, id: string) {
    stopPoll();
    epoch = account.epoch;
    playlist = id;
    serial++;
    read++;
    disposed = false;
    connected = account.status === 'authenticated';
    task.value = undefined;
    image.value = undefined;
    error.value = '';
    warning.value = '';
    busy.value = false;
    // Only a matching successful task read establishes that no task exists.
    readFailed.value = true;
    stopping.value = false;
    changedElsewhere.value = false;
    latest.value = undefined;
    const e = epoch,
      n = serial;
    loading.value = connected;
    if (!connected) return false;
    await refresh(e, n);
    if (live(e, n)) await preview(e, n);
    if (live(e, n)) loading.value = false;
    return live(e, n);
  }
  function schedule(e: string, n: number) {
    stopPoll();
    poll = setTimeout(async () => {
      poll = undefined;
      if (!live(e, n) || !busy.value) return;
      await refresh(e, n);
      if (live(e, n) && busy.value) schedule(e, n);
    }, 500);
  }
  async function choose(revision: string) {
    if (!connected || busy.value || loading.value || readFailed.value) return;
    const previous = task.value;
    if (previous && !coverTerminal(previous) && !coverEditable(previous)) return;
    const e = epoch,
      n = ++serial;
    read++;
    busy.value = true;
    error.value = '';
    warning.value = '';
    try {
      const result = await core.pickPlaylistCover(
        e,
        playlist,
        revision,
        previous && coverEditable(previous) ? previous.id : null,
      );
      if (!live(e, n) || result.epoch !== e) return;
      if (result.value) {
        task.value = result.value;
        image.value = undefined;
        changedElsewhere.value = false;
        latest.value = undefined;
        await preview(e, n);
      }
      if (live(e, n)) return result.value ? 'selected' : 'cancelled';
    } catch (value) {
      if (live(e, n)) {
        problem(value);
        await refresh(e, n);
        if (live(e, n)) return 'failed';
      }
    } finally {
      if (live(e, n)) busy.value = false;
    }
  }
  async function command(command: CoverCommand) {
    if (!connected || busy.value || loading.value || !task.value) return;
    const e = epoch,
      n = ++serial,
      id = task.value.id;
    read++;
    busy.value = true;
    error.value = '';
    warning.value = '';
    schedule(e, n);
    try {
      const result = await core.playlistCoverCommand(e, id, command);
      if (!live(e, n) || result.epoch !== e) return;
      read++;
      task.value = result.value.task;
      error.value = '';
      warning.value = result.value.warning ?? '';
      if (command.kind === 'rebase') {
        changedElsewhere.value = false;
        latest.value = undefined;
      }
      if (result.value.task.phase === 'confirmed' && !announced.has(id)) {
        announced.add(id);
        await saved(result.value);
      }
      return result.value;
    } catch (value) {
      if (live(e, n)) {
        problem(value);
        await refresh(e, n);
      }
    } finally {
      if (live(e, n)) {
        busy.value = false;
        stopPoll();
      }
    }
  }
  async function pause() {
    if (!connected || stopping.value || task.value?.phase !== 'uploading') return;
    const e = epoch,
      n = serial,
      id = task.value.id;
    stopping.value = true;
    try {
      const result = await core.playlistCoverCommand(e, id, { kind: 'pause' });
      if (live(e, n) && result.epoch === e && task.value?.id === id) {
        read++;
        task.value = result.value.task;
      }
    } catch (value) {
      if (live(e, n)) problem(value);
    } finally {
      if (live(e, n)) stopping.value = false;
    }
  }
  async function inspectLatest() {
    if (!connected || busy.value) return;
    const e = epoch,
      n = serial;
    loading.value = true;
    error.value = '';
    try {
      const result = await core.playlistEditor(e, playlist);
      if (live(e, n) && result.epoch === e) latest.value = result.value;
    } catch (value) {
      if (live(e, n)) problem(value);
    } finally {
      if (live(e, n)) loading.value = false;
    }
  }
  function dispose() {
    disposed = true;
    connected = false;
    serial++;
    read++;
    stopPoll();
    task.value = undefined;
    image.value = undefined;
    busy.value = false;
    loading.value = false;
    readFailed.value = false;
  }
  return {
    task,
    image,
    error,
    warning,
    busy,
    loading,
    readFailed,
    stopping,
    changedElsewhere,
    latest,
    connect,
    choose,
    command,
    pause,
    inspectLatest,
    refresh: () => refresh(),
    preview: () => preview(),
    dispose,
  };
}
export type PlaylistCoverModel = ReturnType<typeof usePlaylistCover>;
