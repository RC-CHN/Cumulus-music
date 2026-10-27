import { ref, shallowRef } from 'vue';
import { failure, type AccountView } from '../services/contracts.ts';
import type { SceneFmClient, SceneFmCommand, SceneFmView } from '../services/sceneFm.ts';
export function useSceneFm(core: SceneFmClient, expired: () => void) {
  const view = shallowRef<SceneFmView>(),
    loading = ref(false),
    busy = ref(false),
    error = ref('');
  let epoch: string | undefined,
    serial = 0,
    visible = false,
    disposed = false;
  let timer: ReturnType<typeof setInterval> | undefined;
  let pollFlight: object | undefined;
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
    epoch = next;
    pollFlight = undefined;
    serial++;
    view.value = undefined;
    loading.value = false;
    busy.value = false;
    error.value = '';
    if (visible && epoch) void load(false, true);
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
    if (!epoch || !visible || busy.value || loading.value || disposed || pollFlight) return;
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
  async function load(refresh = false, automatic = false) {
    if (!epoch || busy.value || disposed) return;
    const owner = epoch,
      current = ++serial,
      valid = () => owner === epoch && current === serial && !disposed;
    loading.value = true;
    error.value = '';
    try {
      const cached = await core.sceneFmView(owner, false);
      if (!valid() || cached.epoch !== owner) return;
      accept(cached.value);
      if (refresh || !cached.value.revision) {
        const r = await core.sceneFmView(owner, true);
        if (!valid() || r.epoch !== owner) return;
        accept(r.value);
      }
      if (
        valid() &&
        automatic &&
        visible &&
        view.value?.revision &&
        view.value.autoplay &&
        !view.value.pending
      ) {
        const r = await core.sceneFmCommand(owner, {
          kind: 'automatic',
          revision: view.value.revision,
        });
        if (valid() && r.epoch === owner) accept(r.value);
      }
    } catch (e) {
      if (valid()) handle(e);
    } finally {
      if (valid()) loading.value = false;
    }
  }
  async function command(value: SceneFmCommand) {
    if (!epoch || busy.value || loading.value || disposed) return;
    const owner = epoch,
      current = ++serial,
      valid = () => owner === epoch && current === serial && !disposed;
    busy.value = true;
    error.value = '';
    try {
      const r = await core.sceneFmCommand(owner, value);
      if (valid() && r.epoch === owner) accept(r.value);
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
  function select(id: string) {
    if (view.value?.revision)
      return command({ kind: 'select', revision: view.value.revision, scene_id: id });
  }
  async function show(value: boolean) {
    visible = value;
    if (timer) clearInterval(timer);
    timer = undefined;
    if (!value) {
      if (loading.value) {
        serial++;
        loading.value = false;
      }
      return;
    }
    timer = setInterval(() => void poll(), 1200);
    await load(false, true);
  }
  function dispose() {
    disposed = true;
    serial++;
    if (timer) clearInterval(timer);
  }
  return { view, loading, busy, error, connect, show, load, poll, command, act, select, dispose };
}
export type SceneFmModel = ReturnType<typeof useSceneFm>;
