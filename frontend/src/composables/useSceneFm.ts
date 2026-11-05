import { ref, shallowRef } from 'vue';
import { failure, type AccountView } from '../services/contracts.ts';
import type { SceneFmClient, SceneFmCommand, SceneFmView } from '../services/sceneFm.ts';

export type SceneFmSurface = 'standalone' | 'fallback_home' | 'classic_home' | null;
type ManualSceneFmCommand = Exclude<SceneFmCommand, { kind: 'home_automatic' }>;

export function useSceneFm(core: SceneFmClient, expired: () => void) {
  const view = shallowRef<SceneFmView>(),
    loading = ref(false),
    busy = ref(false),
    error = ref('');
  let epoch: string | undefined,
    serial = 0,
    surface: SceneFmSurface = null,
    disposed = false;
  let timer: ReturnType<typeof setInterval> | undefined;
  let pollFlight: object | undefined;
  let homeEnterAttempted = false;
  let home: { epoch: string; visit: string; automaticSent: boolean } | undefined;

  function release(owner: string, visit: string) {
    void core.sceneFmHomeLeave(owner, visit).catch(() => {
      // A hidden/stale visit must not surface a late error on the new page.
    });
  }
  function retireSurface() {
    if (timer) clearInterval(timer);
    timer = undefined;
    serial++;
    pollFlight = undefined;
    loading.value = false;
    busy.value = false;
    homeEnterAttempted = false;
    if (home) release(home.epoch, home.visit);
    home = undefined;
  }
  function handle(e: unknown) {
    const issue = failure(e);
    error.value = issue.message;
    if (['session_expired', 'profile_changed'].includes(issue.code)) {
      view.value = undefined;
      expired();
    }
  }
  function connect(account: AccountView) {
    const next = account.status === 'authenticated' ? account.epoch : undefined;
    if (next === epoch) return;
    retireSurface();
    surface = null;
    epoch = next;
    view.value = undefined;
    error.value = '';
  }
  function accept(value: SceneFmView) {
    if (
      view.value?.revision &&
      value.revision &&
      BigInt(value.revision) < BigInt(view.value.revision)
    )
      return;
    view.value = value;
  }
  async function poll() {
    if (!epoch || !surface || busy.value || loading.value || disposed || pollFlight) return;
    const flight = {};
    pollFlight = flight;
    const owner = epoch,
      current = serial;
    try {
      const r = await core.sceneFmView(owner, false);
      if (owner === epoch && current === serial && !disposed && r.epoch === owner) accept(r.value);
    } catch (e) {
      if (owner === epoch && current === serial && !disposed) handle(e);
    } finally {
      if (pollFlight === flight) pollFlight = undefined;
    }
  }
  async function load(refresh = false) {
    if (!epoch || !surface || busy.value || loading.value || disposed) return;
    const owner = epoch,
      current = ++serial,
      valid = () => owner === epoch && current === serial && !disposed;
    loading.value = true;
    error.value = '';
    try {
      if (surface === 'classic_home' && !homeEnterAttempted) {
        homeEnterAttempted = true;
        try {
          const entered = await core.sceneFmHomeEnter(owner);
          if (entered.epoch === owner && entered.value) {
            if (!valid()) {
              release(owner, entered.value.visit);
              return;
            }
            home = { epoch: owner, visit: entered.value.visit, automaticSent: false };
          }
        } catch (e) {
          if (!valid()) return;
          if (['session_expired', 'profile_changed'].includes(failure(e).code)) throw e;
          // An unavailable visit never blocks a manual card read or arms autoplay.
        }
      }
      if (!valid()) return;
      const cached = await core.sceneFmView(owner, false);
      if (!valid() || cached.epoch !== owner) return;
      accept(cached.value);
      if (refresh || !cached.value.revision) {
        const r = await core.sceneFmView(owner, true);
        if (!valid() || r.epoch !== owner) return;
        accept(r.value);
      }
      const captured = home;
      if (
        valid() &&
        captured &&
        !captured.automaticSent &&
        surface === 'classic_home' &&
        view.value?.revision &&
        view.value.autoplay &&
        !view.value.pending
      ) {
        // An explicit refresh after directory failure can still make the
        // first attempt. Once dispatched, don't retry this visit's playback.
        captured.automaticSent = true;
        const r = await core.sceneFmCommand(owner, {
          kind: 'home_automatic',
          revision: view.value.revision,
          visit: captured.visit,
        });
        if (valid() && r.epoch === owner) accept(r.value);
      }
    } catch (e) {
      if (valid()) handle(e);
    } finally {
      if (valid()) loading.value = false;
    }
  }
  async function command(value: ManualSceneFmCommand) {
    if (!epoch || !surface || busy.value || loading.value || disposed) return;
    const owner = epoch,
      current = ++serial,
      valid = () => owner === epoch && current === serial && !disposed;
    busy.value = true;
    error.value = '';
    try {
      const r = await core.sceneFmCommand(owner, value);
      if (valid() && r.epoch === owner) {
        accept(r.value);
        return view.value;
      }
    } catch (e) {
      if (valid()) {
        handle(e);
        try {
          const r = await core.sceneFmView(owner, false);
          if (valid() && r.epoch === owner) accept(r.value);
        } catch {
          /* Preserve the original failure. */
        }
      }
    } finally {
      if (valid()) busy.value = false;
    }
  }
  function act(kind: 'play' | 'toggle' | 'cancel') {
    if (view.value?.revision) return command({ kind, revision: view.value.revision });
  }
  function openVoice() {
    const current = view.value;
    if (current?.revision && current.episode)
      return command({
        kind: 'open_voice',
        revision: current.revision,
        voice_id: current.episode.id,
      });
  }
  function select(id: string) {
    if (view.value?.revision)
      return command({ kind: 'select', revision: view.value.revision, scene_id: id });
  }
  async function show(next: SceneFmSurface) {
    if (disposed || next === surface) return;
    retireSurface();
    surface = next;
    if (!epoch || !surface) return;
    timer = setInterval(() => void poll(), 1200);
    await load();
  }
  function dispose() {
    disposed = true;
    retireSurface();
    surface = null;
  }
  return {
    view,
    loading,
    busy,
    error,
    connect,
    show,
    load,
    poll,
    command,
    act,
    openVoice,
    select,
    dispose,
  };
}
export type SceneFmModel = ReturnType<typeof useSceneFm>;
