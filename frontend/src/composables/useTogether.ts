import { ref, shallowRef } from 'vue';
import { useCore } from '../services/core.ts';
import { failure, type AccountView } from '../services/contracts.ts';
import type { TogetherClient, TogetherCommand, TogetherView } from '../services/together.ts';
export function useTogether(
  core: TogetherClient = useCore(),
  expired: () => void = () => {},
  closeFailed: () => void = () => {},
) {
  const view = shallowRef<TogetherView>(),
    epoch = ref(''),
    loading = ref(false),
    busy = ref(false),
    error = ref(''),
    closeError = ref(''),
    notice = ref('');
  let generation = 0,
    request = 0,
    visible = false,
    timer: ReturnType<typeof setTimeout> | undefined;
  let unlisten: (() => void) | undefined;
  function valid(g: number, e: string, n: number) {
    return generation === g && epoch.value === e && request === n;
  }
  function stopTimer() {
    if (timer) clearTimeout(timer);
    timer = undefined;
  }
  function schedule() {
    stopTimer();
    if (epoch.value && (visible || view.value?.attached))
      timer = setTimeout(() => void load(false, true), 1200);
  }
  function issue(value: unknown) {
    const f = failure(value);
    error.value = f.message;
    if (['session_expired', 'profile_changed'].includes(f.code)) {
      dispose();
      expired();
    }
  }
  function dispose() {
    unlisten?.();
    unlisten = undefined;
    closeError.value = '';
    stopTimer();
    generation++;
    request++;
    epoch.value = '';
    view.value = undefined;
    loading.value = false;
    busy.value = false;
  }
  function connect(account: AccountView) {
    dispose();
    error.value = '';
    notice.value = '';
    if (account.status !== 'authenticated') return;
    epoch.value = account.epoch;
    if (core.onTogetherCloseError) {
      const g = generation;
      void core
        .onTogetherCloseError(account.epoch, (message) => {
          if (generation === g) {
            closeError.value = message;
            closeFailed();
          }
        })
        .then((stop) => {
          if (generation === g) unlisten = stop;
          else stop();
        })
        .catch(() => {});
    }
    void load(visible);
  }
  function show(value: boolean) {
    visible = value;
    if (value) void load(true);
    else schedule();
  }
  async function load(refresh = false, quiet = false) {
    if (!epoch.value || busy.value) return;
    stopTimer();
    const g = generation,
      e = epoch.value,
      n = ++request;
    if (!quiet) {
      loading.value = true;
      error.value = '';
    }
    try {
      const result = await core.togetherView(e, refresh);
      if (valid(g, e, n) && result.epoch === e) view.value = result.value;
    } catch (value) {
      if (valid(g, e, n)) {
        issue(value);
        if (refresh) {
          try {
            const local = await core.togetherView(e, false);
            if (valid(g, e, n) && local.epoch === e) view.value = local.value;
          } catch {}
        }
      }
    } finally {
      if (valid(g, e, n)) {
        loading.value = false;
        schedule();
      }
    }
  }
  async function command(command: TogetherCommand) {
    if (!epoch.value || busy.value) return;
    if (
      view.value?.pending.length &&
      ['create', 'attach', 'publish', 'reconnect'].includes(command.kind)
    )
      return;
    stopTimer();
    const g = generation,
      e = epoch.value,
      n = ++request;
    busy.value = true;
    loading.value = false;
    error.value = '';
    notice.value = '';
    try {
      const result = await core.togetherCommand(e, command);
      if (valid(g, e, n) && result.epoch === e) {
        view.value = result.value;
        closeError.value = '';
        notice.value =
          command.kind === 'end'
            ? '已退出房间'
            : command.kind === 'reconcile'
              ? '已核对服务端状态'
              : command.kind === 'restore_local'
                ? '已恢复播放设置'
                : '';
      }
    } catch (value) {
      if (valid(g, e, n)) {
        issue(value);
        try {
          const local = await core.togetherView(e, false);
          if (valid(g, e, n) && local.epoch === e) view.value = local.value;
        } catch {}
      }
    } finally {
      if (valid(g, e, n)) {
        busy.value = false;
        schedule();
      }
    }
  }
  return {
    view,
    epoch,
    loading,
    busy,
    error,
    closeError,
    notice,
    connect,
    show,
    dispose,
    load,
    command,
  };
}
export type TogetherModel = ReturnType<typeof useTogether>;
