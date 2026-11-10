import { computed, ref, shallowRef, watch } from 'vue';
import { failure, type AccountView } from '../services/contracts.ts';
import { useCore } from '../services/core.ts';
import type {
  EpisodeView,
  PodcastLikedClient,
  PodcastLikedPage,
  PodcastLikedPageTarget,
  PodcastLikedVisit,
} from '../services/podcast.ts';

const changed = {
  code: 'snapshot_changed',
  message: '列表已更新，请刷新后重试',
  retryable: true,
};
const lease = ({ visit, revision }: PodcastLikedVisit): PodcastLikedVisit => ({ visit, revision });
const sameVisit = (a: PodcastLikedVisit, b: PodcastLikedVisit) =>
  a.visit === b.visit && a.revision === b.revision;

/** Display-only liked programmes. Never writes the full liked membership store. */
export function usePodcastLiked(
  account: () => AccountView,
  expired: () => void,
  core: PodcastLikedClient = useCore(),
) {
  const page = shallowRef<PodcastLikedPage & { epoch: string }>();
  const count = ref<number | null>(null);
  const issue = ref('');
  const scrollTop = ref(0);
  const controlPending = ref(false);
  const pagePending = ref(false);
  const pending = computed(() => controlPending.value || pagePending.value);
  let visible = false;
  let activation = 0;
  let ownerGeneration = 0;
  let countRequest = 0;
  let nativeVisit: PodcastLikedVisit | undefined;
  let saved: { visit: PodcastLikedVisit; scrollTop: number } | undefined;
  let controls = Promise.resolve();
  let retryAction: (() => Promise<void>) | undefined;

  watch(
    [() => account().epoch, () => account().status],
    () => {
      activation++;
      ownerGeneration++;
      countRequest++;
      visible = false;
      page.value = undefined;
      count.value = null;
      issue.value = '';
      scrollTop.value = 0;
      controlPending.value = false;
      pagePending.value = false;
      nativeVisit = undefined;
      saved = undefined;
      controls = Promise.resolve();
      retryAction = undefined;
    },
    { flush: 'sync' },
  );

  function owns(epoch: string) {
    const current = account();
    return !!epoch && current.epoch === epoch && current.status === 'authenticated';
  }

  function live(epoch: string, intent: number) {
    return visible && activation === intent && owns(epoch);
  }

  function serialize<T>(action: () => Promise<T>) {
    // Native route controls stay in navigation order. Held page reads run
    // separately so they cannot delay leaving or returning to the route.
    const result = controls.then(action);
    controls = result.then(
      () => {},
      () => {},
    );
    return result;
  }

  function authenticationIssue(error: unknown) {
    const detail = failure(error);
    if (detail.code === 'session_expired' || detail.code === 'profile_changed') expired();
    return detail;
  }

  function publish(value: PodcastLikedPage, epoch: string, episodes = value.episodes) {
    if (value.loadedCount !== episodes.length) throw changed;
    page.value = { ...value, epoch, episodes };
    if (value.count !== null) {
      countRequest++;
      count.value = value.count;
    }
  }

  async function enter(restore: boolean) {
    const { epoch } = account();
    if (!owns(epoch)) return;
    const intent = ++activation;
    const owner = ownerGeneration;
    visible = true;
    page.value = undefined;
    issue.value = '';
    retryAction = undefined;
    controlPending.value = true;
    pagePending.value = false;
    scrollTop.value = 0;

    const first = await serialize(async () => {
      if (!live(epoch, intent)) return false;
      try {
        let reply;
        let restored = false;
        const previous = restore ? saved : undefined;
        if (previous) {
          try {
            reply = await core.podcastLikedPage(epoch, { kind: 'restore', ...previous.visit });
          } catch (error) {
            if (!live(epoch, intent)) return false;
            if (failure(error).code !== 'snapshot_changed') throw error;
            // Only a known obsolete native prefix makes Back a new entry.
          }
          if (reply) {
            if (
              reply.epoch !== epoch ||
              reply.value.visit !== previous.visit.visit ||
              reply.value.revision === previous.visit.revision
            )
              throw changed;
            restored = true;
          }
        }
        if (!reply) {
          reply = await core.podcastLikedPage(epoch, { kind: 'open' });
          if (reply.epoch !== epoch) throw changed;
        }
        if (owner !== ownerGeneration || !owns(epoch)) return false;
        // Even a superseded same-account Open/Restore must leave its receipt
        // for the already queued Suspend, rather than orphan an active visit.
        nativeVisit = lease(reply.value);
        saved = undefined;
        if (!live(epoch, intent)) return false;
        if (restored && previous) scrollTop.value = previous.scrollTop;
        publish(reply.value, epoch);
        return !restored;
      } catch (error) {
        if (live(epoch, intent)) {
          issue.value = authenticationIssue(error).message;
          retryAction = () => enter(restore);
        }
        return false;
      } finally {
        if (live(epoch, intent)) controlPending.value = false;
      }
    });
    if (first && live(epoch, intent)) await next();
  }

  function suspend(position = scrollTop.value) {
    const { epoch } = account();
    if (!visible || !owns(epoch)) return controls;
    visible = false;
    activation++;
    countRequest++;
    page.value = undefined;
    issue.value = '';
    retryAction = undefined;
    controlPending.value = false;
    pagePending.value = false;
    const previousScroll = Number.isFinite(position) ? Math.max(0, position) : 0;
    const owner = ownerGeneration;
    const current = () => owner === ownerGeneration && owns(epoch);
    return serialize(async () => {
      if (!current() || !nativeVisit) return;
      const target = nativeVisit;
      nativeVisit = undefined;
      saved = undefined;
      try {
        const reply = await core.podcastLikedPage(epoch, { kind: 'suspend', ...target });
        if (current() && reply.epoch === epoch && sameVisit(reply.value, target)) {
          saved = { visit: target, scrollTop: previousScroll };
        }
      } catch (error) {
        if (current()) authenticationIssue(error);
        // A missing/invalid receipt will make the next explicit Back open fresh.
      }
    });
  }

  async function next() {
    const shown = page.value;
    const intent = activation;
    if (!shown?.continuation || !live(shown.epoch, intent) || pending.value || issue.value) return;
    pagePending.value = true;
    const epoch = shown.epoch;
    const current = () => live(epoch, intent) && page.value === shown;
    try {
      const reply = await core.podcastLikedPage(epoch, {
        kind: 'next',
        ...lease(shown),
        continuation: shown.continuation,
      });
      if (!current()) return;
      if (
        reply.epoch !== epoch ||
        !sameVisit(reply.value, shown) ||
        reply.value.continuation === shown.continuation
      )
        throw changed;
      publish(reply.value, epoch, [...shown.episodes, ...reply.value.episodes]);
    } catch (error) {
      if (current()) {
        const detail = authenticationIssue(error);
        issue.value = detail.message;
        retryAction = detail.code === 'snapshot_changed' ? () => enter(false) : next;
      }
    } finally {
      if (live(epoch, intent)) pagePending.value = false;
    }
  }

  function retry() {
    const action = retryAction;
    if (!action || !visible || pending.value) return;
    retryAction = undefined;
    issue.value = '';
    return action();
  }

  async function readCount() {
    const { epoch } = account();
    if (!owns(epoch)) return;
    const intent = activation;
    const request = ++countRequest;
    const current = () => owns(epoch) && intent === activation && request === countRequest;
    try {
      const reply = await core.podcastLikedCount(epoch);
      if (
        current() &&
        reply.epoch === epoch &&
        Number.isSafeInteger(reply.value) &&
        reply.value >= 0
      )
        count.value = reply.value;
    } catch (error) {
      if (current()) authenticationIssue(error);
      // The optional count cannot fail the active programme list.
    }
  }

  function captureProgram(row: EpisodeView) {
    const shown = page.value;
    if (!shown?.episodes.includes(row)) return;
    const { epoch } = shown;
    const intent = activation;
    const target: PodcastLikedPageTarget = { ...lease(shown), voiceId: row.id };
    const current = () => {
      const value = page.value;
      return live(epoch, intent) && !!value && sameVisit(value, target);
    };
    if (current()) return { epoch, target, live: current };
  }

  async function resolveProgram(row: EpisodeView) {
    const captured = captureProgram(row);
    if (!captured) return;
    try {
      const reply = await core.podcastLikedPageProgram(captured.epoch, captured.target);
      if (captured.live() && reply.epoch === captured.epoch && reply.value.id === row.id)
        return reply.value;
    } catch (error) {
      if (captured.live()) authenticationIssue(error);
    }
  }

  return {
    page,
    count,
    pending,
    issue,
    scrollTop,
    open: () => enter(false),
    suspend,
    restore: () => enter(true),
    next,
    retry,
    readCount,
    captureProgram,
    resolveProgram,
  };
}
export type PodcastLikedModel = ReturnType<typeof usePodcastLiked>;
