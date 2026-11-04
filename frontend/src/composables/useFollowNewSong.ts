import { computed, ref, shallowRef } from 'vue';
import { failure } from '../services/contracts.ts';
import type { TrackView } from '../services/contracts.ts';
import type { FollowNewSongClient, FollowRelease } from '../services/followNewSong.ts';

const GROUP_PAGES = 4;

export function useFollowNewSong(core: FollowNewSongClient, expired: () => void) {
  const pages = shallowRef<FollowRelease[][]>([]);
  const latest = shallowRef<TrackView[]>([]);
  const latestRevision = ref<string | null>(null);
  const latestVisitTime = ref<number | null>(null);
  const cursor = ref<number | null>(null);
  const pending = ref(false);
  const initial = ref(false);
  const latestPending = ref(false);
  const error = ref('');
  const latestError = ref('');
  const group = ref(0);
  let current: string | null = null;
  let generation = 0;
  const releases = computed(() =>
    pages.value.slice(group.value * GROUP_PAGES, (group.value + 1) * GROUP_PAGES).flat(),
  );
  const groups = computed(() => Math.max(1, Math.ceil(pages.value.length / GROUP_PAGES)));
  const autoMore = computed(
    () =>
      cursor.value !== null &&
      pages.value.length % GROUP_PAGES !== 0 &&
      group.value === groups.value - 1,
  );

  function clear() {
    current = null;
    ++generation;
    pages.value = [];
    latest.value = [];
    latestRevision.value = null;
    latestVisitTime.value = null;
    cursor.value = null;
    pending.value = false;
    initial.value = false;
    latestPending.value = false;
    error.value = '';
    latestError.value = '';
    group.value = 0;
  }

  function report(reason: unknown, target: typeof error) {
    const issue = failure(reason);
    target.value = issue.message;
    if (issue.code === 'session_expired' || issue.code === 'profile_changed') expired();
  }

  async function latestRead() {
    const epoch = current;
    const ticket = generation;
    if (epoch === null || latestPending.value) return;
    latestPending.value = true;
    latestError.value = '';
    try {
      const reply = await core.followedNewSongLatest(epoch);
      if (ticket === generation && reply.epoch === epoch) {
        latest.value = reply.value.tracks;
        latestRevision.value = reply.value.revision;
      }
    } catch (reason) {
      if (ticket === generation) report(reason, latestError);
    } finally {
      if (ticket === generation) latestPending.value = false;
    }
  }

  async function read() {
    const epoch = current;
    const ticket = generation;
    const first = initial.value;
    const start = first ? null : cursor.value;
    if (epoch === null || pending.value || (!first && start === null)) return;
    pending.value = true;
    error.value = '';
    try {
      const reply = await core.followedNewSongPage(epoch, start);
      if (ticket !== generation || reply.epoch !== epoch) return;
      const data = reply.value;
      pages.value = first ? [data.releases] : [...pages.value, data.releases];
      group.value = groups.value - 1;
      if (first) latestVisitTime.value = data.latestVisitTime;
      initial.value = false;
      cursor.value = data.nextTimestamp;
    } catch (reason) {
      if (ticket === generation) report(reason, error);
    } finally {
      if (ticket === generation) pending.value = false;
    }
  }

  function connect(epoch: string | null) {
    clear();
    if (epoch === null) return;
    current = epoch;
    initial.value = true;
    // The official firstFetch first dispatches fetch50Latest, then requests
    // the release list; each result settles independently.
    void latestRead();
    void read();
  }

  function chooseGroup(value: number) {
    if (value >= 0 && value < groups.value) group.value = value;
  }

  return {
    releases,
    latest,
    latestRevision,
    latestVisitTime,
    cursor,
    pending,
    initial,
    latestPending,
    error,
    latestError,
    group,
    groups,
    autoMore,
    read,
    latestRead,
    connect,
    chooseGroup,
    clear,
  };
}
