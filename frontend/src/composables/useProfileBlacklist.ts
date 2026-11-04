import { onScopeDispose, ref, shallowRef, watch } from 'vue';
import { failure } from '../services/contracts';
import { useCore } from '../services/core';
import {
  blacklistMessage,
  type ProfileBlacklistClient,
  type ProfileBlacklistCommand,
  type ProfileBlacklistList,
} from '../services/profileBlacklist';

export function useProfileBlacklist(
  epoch: () => string | undefined,
  expired: () => void,
  core: ProfileBlacklistClient = useCore(),
) {
  const list = shallowRef<ProfileBlacklistList>();
  const busy = ref(false);
  const issue = ref('');
  const notice = ref('');
  let generation = 0;
  function reset() {
    generation++;
    list.value = undefined;
    busy.value = false;
    issue.value = notice.value = '';
  }
  watch(epoch, reset, { flush: 'sync' });
  onScopeDispose(reset);

  async function run<T>(operation: (owner: string, current: () => boolean) => Promise<T>) {
    const owner = epoch();
    if (!owner || busy.value) return;
    const ticket = ++generation;
    const current = () => epoch() === owner && ticket === generation;
    busy.value = true;
    issue.value = notice.value = '';
    try {
      return await operation(owner, current);
    } catch (value) {
      if (!current()) return;
      const error = failure(value);
      issue.value = error.message;
      if (error.code === 'session_expired' || error.code === 'profile_changed') expired();
    } finally {
      if (current()) busy.value = false;
    }
  }

  function load() {
    return run(async (owner, current) => {
      const reply = await core.profileBlacklistList(owner);
      if (current() && reply.epoch === owner) list.value = reply.value;
    });
  }

  function act(id: string, command: ProfileBlacklistCommand) {
    return run(async (owner, current) => {
      const reply = await core.profileBlacklist(owner, id, command);
      if (!current() || reply.epoch !== owner) return;
      const result = reply.value;
      if (result.users || list.value)
        list.value = {
          users: result.users ?? list.value?.users ?? [],
          pending: result.pending,
        };
      notice.value = blacklistMessage[result.status];
      return result;
    });
  }
  return { list, busy, issue, notice, load, act, reset };
}
