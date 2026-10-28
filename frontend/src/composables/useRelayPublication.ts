import { computed, ref, shallowRef } from 'vue';
import { failure, type AccountData, type AccountView } from '../services/contracts.ts';
import type {
  RelayPublicationClient,
  RelayPublicationCommand,
  RelayPublicationView,
} from '../services/relayPublication.ts';
import { useCore } from '../services/core.ts';
export function useRelayPublication(core: RelayPublicationClient = useCore(), expired = () => {}) {
  const page = shallowRef<RelayPublicationView>(),
    busy = ref(false),
    loading = ref(false),
    error = ref('');
  const unknown = shallowRef<{ id: string; blocked_id: string }>();
  let epoch: string | undefined,
    serial = 0,
    readSerial = 0,
    reading = false,
    visibilityGeneration = 0,
    disposed = false,
    visible = false,
    requested: string | null = null;
  let timer: ReturnType<typeof setTimeout> | undefined;
  const canResynchronize = computed(
    () =>
      !!epoch &&
      !!page.value?.canResynchronize &&
      !busy.value &&
      !loading.value &&
      !unknown.value &&
      !error.value,
  );
  function connect(account: AccountView) {
    const next = account.status === 'authenticated' ? account.epoch : undefined;
    if (next === epoch) return;
    epoch = next;
    serial++;
    readSerial++;
    reading = false;
    page.value = undefined;
    unknown.value = undefined;
    requested = null;
    busy.value = false;
    loading.value = false;
    error.value = '';
    if (visible) void read();
  }
  function accept(value: AccountData<RelayPublicationView>, owner: string) {
    if (value.epoch !== owner)
      throw { code: 'invalid_response', message: '同步账号已变化，请重新读取' };
    page.value = value.value;
    if (unknown.value && value.value.requested?.id === unknown.value.id) unknown.value = undefined;
  }
  function failed(value: unknown) {
    const issue = failure(value);
    error.value = issue.message;
    if (['session_expired', 'profile_changed', 'stale'].includes(issue.code)) {
      epoch = undefined;
      serial++;
      readSerial++;
      reading = false;
      page.value = undefined;
      unknown.value = undefined;
      requested = null;
      busy.value = false;
      loading.value = false;
      expired();
      return true;
    }
    return false;
  }
  async function read(silent = false) {
    if (!epoch || disposed || busy.value || reading) return;
    const owner = epoch,
      current = ++serial,
      readId = ++readSerial;
    const active = () => !disposed && epoch === owner && serial === current;
    reading = true;
    loading.value = !silent || !page.value;
    try {
      const value = await core.relayPublicationView(owner, unknown.value?.id ?? requested);
      if (active()) {
        accept(value, owner);
        error.value = '';
      }
    } catch (e) {
      if (active()) failed(e);
    } finally {
      if (readSerial === readId) reading = false;
      if (active()) loading.value = false;
    }
  }
  async function command(value: RelayPublicationCommand) {
    if (!epoch || disposed || busy.value || loading.value) return false;
    const owner = epoch,
      current = ++serial;
    const active = () => !disposed && epoch === owner && serial === current;
    busy.value = true;
    error.value = '';
    requested = value.id;
    try {
      const result = await core.relayPublicationCommand(owner, value);
      if (!active()) return false;
      accept(result, owner);
      return true;
    } catch (e) {
      if (!active() || failed(e)) return false;
      if (value.kind === 'resynchronize')
        unknown.value = { id: value.id, blocked_id: value.blocked_id };
      try {
        const result = await core.relayPublicationView(owner, value.id);
        if (active()) {
          accept(result, owner);
          if (result.value.requested?.id === value.id) error.value = '';
        }
      } catch {
        /* preserve unknown; never resend */
      }
      return false;
    } finally {
      if (active()) busy.value = false;
    }
  }
  async function resynchronize(blockedId: string) {
    if (!canResynchronize.value || page.value?.blocked?.id !== blockedId) return false;
    return command({ kind: 'resynchronize', id: crypto.randomUUID(), blocked_id: blockedId });
  }
  async function resumePrepared() {
    const row = page.value?.recovering;
    if (unknown.value || page.value?.recordFailed || row?.phase !== 'prepared' || !row.recoveryOf)
      return false;
    return command({ kind: 'resynchronize', id: row.id, blocked_id: row.recoveryOf });
  }
  async function resolveUnknown() {
    if (!unknown.value) return false;
    return command({ kind: 'cancel_unseen', ...unknown.value });
  }
  function setVisible(value: boolean) {
    visible = value;
    const generation = ++visibilityGeneration;
    if (timer) clearTimeout(timer);
    timer = undefined;
    if (!visible || disposed) return;
    const poll = async () => {
      await read(true);
      if (visible && !disposed && generation === visibilityGeneration)
        timer = setTimeout(poll, 2000);
    };
    void poll();
  }
  function dispose() {
    disposed = true;
    serial++;
    if (timer) clearTimeout(timer);
  }
  return {
    page,
    busy,
    loading,
    error,
    unknown,
    canResynchronize,
    connect,
    read,
    resynchronize,
    resumePrepared,
    resolveUnknown,
    setVisible,
    dispose,
  };
}
export type RelayPublicationModel = ReturnType<typeof useRelayPublication>;
