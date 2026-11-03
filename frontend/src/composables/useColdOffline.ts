import { computed, ref, shallowRef } from 'vue';
import type {
  ColdOfflineClient,
  ColdOfflinePage,
  ColdOfflinePlayback,
} from '../services/coldOffline';

export function useColdOffline(core: ColdOfflineClient, pollAfter = 750) {
  const page = shallowRef<ColdOfflinePage>();
  const playback = shallowRef<ColdOfflinePlayback>();
  const currentTitle = ref('');
  const error = ref('');
  const busy = ref(false);
  const closeFailed = ref(false);
  let eligible = false;
  let generation = 0;
  let hasLease = false;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let command: Promise<void> | undefined;
  let closing: Promise<void> | undefined;

  function clear() {
    clearTimeout(timer);
    page.value = undefined;
    playback.value = undefined;
    currentTitle.value = '';
  }
  function leave() {
    generation++;
    clear();
    error.value = '';
    if (closing || !hasLease) return;
    busy.value = true;
    const pending = command;
    closing = (async () => {
      await pending;
      try {
        await core.coldOfflineClose();
        hasLease = false;
        closeFailed.value = false;
      } catch {
        closeFailed.value = true;
      } finally {
        closing = undefined;
        busy.value = false;
      }
    })();
  }
  function setEligible(value: boolean) {
    if (value === eligible) return;
    eligible = value;
    if (!value) leave();
  }
  function schedule() {
    clearTimeout(timer);
    if (page.value && eligible && !closing) timer = setTimeout(poll, pollAfter);
  }
  function sync(next: ColdOfflinePlayback) {
    if (next.phase === 'idle') currentTitle.value = '';
    else if (next.playingId !== playback.value?.playingId)
      currentTitle.value =
        page.value?.items.find((item) => item.id === next.playingId)?.title ?? '';
    playback.value = next;
  }
  function run(
    work: (current: () => boolean) => Promise<void>,
    foreground = true,
  ): Promise<void> | undefined {
    if (!eligible || closing || closeFailed.value) return;
    const owner = generation;
    if (command) {
      if (!foreground || busy.value) return;
      busy.value = true;
      return command.then(() => {
        if (eligible && generation === owner) return run(work);
      });
    }
    const current = () => eligible && generation === owner;
    if (foreground) {
      busy.value = true;
      error.value = '';
    }
    clearTimeout(timer);
    command = Promise.resolve()
      .then(() => {
        if (current()) return work(current);
      })
      .finally(() => {
        command = undefined;
        if (current()) {
          busy.value = false;
          schedule();
        }
      });
    return command;
  }
  function revoke() {
    leave();
    error.value = '离线访问已结束，请重新连接后再试。';
  }
  async function status(current: () => boolean) {
    try {
      const next = await core.coldOfflineStatus();
      if (current()) sync(next);
    } catch {
      if (current()) revoke();
    }
  }
  function poll() {
    void run(status, false);
  }
  function open() {
    if (page.value || hasLease || command || !eligible) return;
    hasLease = true; // Also close after an uncertain open reply.
    return run(async (current) => {
      try {
        const next = await core.coldOfflineOpen();
        if (current()) {
          page.value = next;
          playback.value = { phase: 'idle', playingId: null };
        }
      } catch {
        if (current()) {
          leave();
          error.value = '当前无法打开离线下载。请重新连接后再试。';
        }
      }
    });
  }
  function next() {
    if (!page.value?.hasMore) return;
    return run(async (current) => {
      try {
        const value = await core.coldOfflineNext();
        if (current()) page.value = value;
      } catch {
        if (current()) {
          // A lost reply may have moved the native cursor. Reopen at the
          // first page explicitly rather than silently skipping records.
          leave();
          error.value = '无法读取下一页，请重新打开离线下载。';
        }
      }
    });
  }
  function play(id: string) {
    if (!page.value?.items.some((item) => item.id === id)) return;
    return run(async (current) => {
      try {
        const value = await core.coldOfflinePlay(id);
        if (current()) sync(value);
      } catch {
        if (current()) {
          error.value = '无法播放这首离线歌曲。';
          await status(current);
        }
      }
    });
  }
  function stop() {
    if (playback.value?.phase !== 'playing') return;
    return run(async (current) => {
      try {
        const value = await core.coldOfflineStop();
        if (current()) sync(value);
      } catch {
        if (current()) {
          error.value = '无法确认播放已停止，可退出离线模式。';
          await status(current);
        }
      }
    });
  }
  return {
    page: computed(() => page.value),
    playback: computed(() => playback.value),
    currentTitle: computed(() => currentTitle.value),
    error: computed(() => error.value),
    busy: computed(() => busy.value),
    closeFailed: computed(() => closeFailed.value),
    setEligible,
    open,
    next,
    play,
    stop,
    leave,
  };
}
