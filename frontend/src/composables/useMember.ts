import { ref, shallowRef } from 'vue';
import type { MemberClient, MemberQuery, MemberRights, MemberView } from '../services/member.ts';
import { failure, type AccountView } from '../services/contracts.ts';
import { useCore } from '../services/core.ts';
export function useMember(core: MemberClient = useCore(), expired = () => {}) {
  const view = shallowRef<MemberView>();
  const rights = shallowRef<MemberRights>();
  const query = shallowRef<MemberQuery>({ kind: 'home' });
  const loading = ref(false),
    rightsLoading = ref(false),
    error = ref(''),
    rightsError = ref(''),
    linkBusy = ref(false);
  let epoch: string | undefined,
    serial = 0,
    rightsSerial = 0,
    linkSerial = 0,
    disposed = false,
    visible = false;
  let timer: ReturnType<typeof setTimeout> | undefined;
  function stopTimer() {
    if (timer) clearTimeout(timer);
    timer = undefined;
  }
  function connect(account: AccountView) {
    const next = account.status === 'authenticated' ? account.epoch : undefined;
    if (next === epoch) return;
    epoch = next;
    serial++;
    rightsSerial++;
    linkSerial++;
    stopTimer();
    view.value = undefined;
    rights.value = undefined;
    error.value = '';
    rightsError.value = '';
    loading.value = false;
    rightsLoading.value = false;
    linkBusy.value = false;
    query.value = { kind: 'home' };
    if (visible && epoch) {
      void open(query.value);
      void loadRights();
    }
  }
  function sessionError(e: unknown) {
    const issue = failure(e);
    if (['session_expired', 'profile_changed'].includes(issue.code)) {
      epoch = undefined;
      serial++;
      rightsSerial++;
      linkSerial++;
      loading.value = false;
      rightsLoading.value = false;
      linkBusy.value = false;
      view.value = undefined;
      rights.value = undefined;
      stopTimer();
      expired();
    }
    return issue.message;
  }
  async function open(target: MemberQuery, refresh = false) {
    const same = JSON.stringify(target) === JSON.stringify(query.value);
    if (!refresh && same && loading.value) return;
    const owner = epoch,
      n = ++serial;
    query.value = target;
    if (!same) view.value = undefined;
    error.value = '';
    if (!owner || disposed) return;
    loading.value = true;
    const active = () => !disposed && owner === epoch && n === serial;
    try {
      if (!refresh) {
        try {
          const saved = await core.memberPage(owner, target, false);
          if (!active()) return;
          if (saved.epoch !== owner) return;
          view.value = saved.value;
          if (saved.value.fresh) return;
        } catch (e) {
          if (!active()) return;
          if (['session_expired', 'profile_changed'].includes(failure(e).code)) throw e;
        }
      }
      if (!active()) return;
      const fresh = await core.memberPage(owner, target, true);
      if (active() && fresh.epoch === owner) view.value = fresh.value;
    } catch (e) {
      if (active()) error.value = sessionError(e);
    } finally {
      if (active()) loading.value = false;
    }
  }
  async function loadRights(refresh = false) {
    if (rightsLoading.value || !epoch || disposed) return;
    const owner = epoch,
      n = ++rightsSerial;
    rightsLoading.value = true;
    rightsError.value = '';
    stopTimer();
    const active = () => !disposed && owner === epoch && n === rightsSerial;
    try {
      if (!refresh) {
        try {
          const cached = await core.memberRights(owner, false);
          if (!active()) return;
          if (cached.epoch !== owner) return;
          rights.value = cached.value;
        } catch (e) {
          if (!active()) return;
          if (['session_expired', 'profile_changed'].includes(failure(e).code)) throw e;
        }
      }
      if (!active()) return;
      const fresh = await core.memberRights(owner, true);
      if (active() && fresh.epoch === owner) rights.value = fresh.value;
    } catch (e) {
      if (active()) rightsError.value = sessionError(e);
    } finally {
      if (active()) {
        rightsLoading.value = false;
        if (visible)
          timer = setTimeout(
            () => void loadRights(true),
            rightsError.value
              ? 60_000
              : Math.min(300_000, Math.max(1_000, rights.value?.refreshAfterMs ?? 300_000)),
          );
      }
    }
  }
  async function openExternal(key: string) {
    if (linkBusy.value || !epoch || !view.value) return;
    const owner = epoch,
      n = ++linkSerial;
    linkBusy.value = true;
    error.value = '';
    try {
      await core.openMemberLink(owner, view.value.revision, key);
    } catch (e) {
      if (!disposed && owner === epoch && n === linkSerial) error.value = sessionError(e);
    } finally {
      if (!disposed && owner === epoch && n === linkSerial) linkBusy.value = false;
    }
  }
  function show(value: boolean) {
    visible = value;
    stopTimer();
    if (value) {
      void open(query.value);
      void loadRights();
    }
  }
  function dispose() {
    disposed = true;
    serial++;
    rightsSerial++;
    linkSerial++;
    stopTimer();
  }
  return {
    view,
    rights,
    query,
    loading,
    rightsLoading,
    error,
    rightsError,
    linkBusy,
    connect,
    show,
    open,
    openExternal,
    loadRights,
    refresh: () => open(query.value, true),
    dispose,
  };
}
