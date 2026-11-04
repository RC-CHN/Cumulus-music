import { reactive, ref } from 'vue';
import { failure } from '../services/contracts.ts';
import type { ProfilePodcastClient, ProfilePodcastPage } from '../services/profilePodcasts';

export type ProfilePodcastGroup = 'created' | 'collected';
export const PROFILE_PODCAST_PAGE_SIZE = 20;
export const PROFILE_PODCAST_GROUPS = ['created', 'collected'] as const;

function emptyGroup() {
  return {
    page: null as ProfilePodcastPage | null,
    pending: false,
    issue: '',
    requested: 0,
    display: 'grid' as 'grid' | 'list',
    localOffset: 0,
  };
}

export function useProfilePodcasts(core: ProfilePodcastClient, expired: () => void) {
  const groups = reactive({ created: emptyGroup(), collected: emptyGroup() });
  const unavailable = ref(false);
  const opener = ref<{ group: ProfilePodcastGroup; id: string } | null>(null);
  const managing = ref(false);
  const managementIssue = ref('');
  let owner = '';
  let uid = '';
  let generation = 0;

  function connect(epoch: string, target: string) {
    if (epoch === owner && target === uid) return;
    owner = epoch;
    uid = target;
    generation++;
    for (const kind of PROFILE_PODCAST_GROUPS) Object.assign(groups[kind], emptyGroup());
    unavailable.value = false;
    opener.value = null;
    managing.value = false;
    managementIssue.value = '';
  }

  async function read(kind: ProfilePodcastGroup, offset = 0) {
    const state = groups[kind];
    if (!owner || !uid || state.pending || unavailable.value) return;
    const current = generation;
    state.pending = true;
    state.requested = offset;
    state.issue = '';
    try {
      const response = await core.profilePodcasts(
        owner,
        uid,
        kind === 'created' ? { kind, offset } : { kind },
      );
      if (current !== generation || response.epoch !== owner) return;
      if (response.value === null) {
        generation++;
        for (const kind of PROFILE_PODCAST_GROUPS) Object.assign(groups[kind], emptyGroup());
        unavailable.value = true;
        opener.value = null;
      } else {
        state.page = response.value;
      }
    } catch (error) {
      if (current !== generation) return;
      const issue = failure(error);
      if (issue.code === 'session_expired' || issue.code === 'profile_changed') expired();
      state.issue = '暂时无法读取播客。';
    } finally {
      if (current === generation) state.pending = false;
    }
  }

  function ensure() {
    // Both independent groups start on entry. The favourite collection is a
    // single official request; its presentation pages never invoke native IPC.
    for (const kind of ['collected', 'created'] as const) {
      if (!groups[kind].page && !groups[kind].issue) void read(kind);
    }
  }

  async function manage() {
    if (!owner || !uid || managing.value || unavailable.value) return;
    const current = generation;
    managing.value = true;
    managementIssue.value = '';
    try {
      await core.openProfilePodcastManagement(owner, uid);
    } catch (error) {
      if (current !== generation) return;
      const issue = failure(error);
      if (issue.code === 'session_expired' || issue.code === 'profile_changed') expired();
      managementIssue.value = '无法打开浏览器。';
    } finally {
      if (current === generation) managing.value = false;
    }
  }

  return { groups, unavailable, opener, managing, managementIssue, connect, read, ensure, manage };
}
