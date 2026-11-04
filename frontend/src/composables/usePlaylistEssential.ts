import { computed, ref, shallowRef } from 'vue';
import {
  failure,
  type CoreClient,
  type PlaylistEssentialCategory,
  type PlaylistEssentialPage,
} from '../services/contracts.ts';

const BATCH_PAGES = 3;
interface Snapshot extends PlaylistEssentialPage {
  pages: number;
  seen: string[];
}
export function usePlaylistEssential(
  core: Pick<CoreClient, 'playlistEssentialCategories' | 'playlistEssentialPage'>,
  expired: () => void,
) {
  const tags = shallowRef<PlaylistEssentialCategory[]>();
  const page = shallowRef<Snapshot>();
  const pending = ref(false),
    error = ref(''),
    restart = ref(false);
  const tagsPending = ref(false),
    tagsError = ref('');
  const nextBatch = computed(
    () => (page.value?.pages ?? 0) >= BATCH_PAGES && !!page.value?.continuation,
  );
  const saved = new Map<string, Snapshot>();
  const openers = new Map<string, string>();
  let epoch = '',
    active = '',
    sequence = 0,
    tagSequence = 0;
  let failedRefresh = false;

  async function loadTags() {
    if (!epoch || !active || tags.value || tagsPending.value) return;
    const owner = epoch,
      current = ++tagSequence;
    tagsPending.value = true;
    tagsError.value = '';
    try {
      const reply = await core.playlistEssentialCategories(owner);
      if (current === tagSequence && owner === epoch && reply.epoch === owner)
        tags.value = reply.value;
    } catch (value) {
      if (current !== tagSequence || owner !== epoch) return;
      const issue = failure(value);
      tagsError.value = issue.message;
      if (['session_expired', 'profile_changed'].includes(issue.code)) expired();
    } finally {
      if (current === tagSequence) tagsPending.value = false;
    }
  }

  async function load(refresh = false) {
    if (!epoch || !active || pending.value || (!refresh && page.value && !page.value.continuation))
      return;
    const current = ++sequence,
      category = active,
      owner = epoch;
    const previous = refresh ? undefined : page.value;
    const batch = !previous || nextBatch.value;
    const live = () => sequence === current && active === category && epoch === owner;
    pending.value = true;
    error.value = '';
    restart.value = false;
    failedRefresh = refresh;
    try {
      const reply = await core.playlistEssentialPage(
        owner,
        category,
        previous?.continuation ?? undefined,
      );
      if (!live() || reply.epoch !== owner) return;
      const next = reply.value.continuation;
      if (next && previous?.seen.includes(next)) {
        restart.value = true;
        error.value = '精品歌单未继续更新，请重新加载';
        return;
      }
      const items = batch ? [] : [...previous.items];
      const known = new Set(items.map((item) => item.id));
      for (const item of reply.value.items) {
        if (known.has(item.id)) continue;
        known.add(item.id);
        items.push(item);
      }
      page.value = {
        items,
        continuation: next,
        pages: batch ? 1 : previous.pages + 1,
        seen: [...(previous?.seen ?? []), ...(next ? [next] : [])].slice(-64),
      };
      if (batch) openers.delete(category);
      saved.delete(category);
      saved.set(category, page.value);
      if (saved.size > 8) {
        const oldest = saved.keys().next().value!;
        saved.delete(oldest);
        openers.delete(oldest);
      }
    } catch (value) {
      if (!live()) return;
      const issue = failure(value);
      restart.value = issue.code === 'stale';
      error.value = issue.message;
      if (['session_expired', 'profile_changed'].includes(issue.code)) expired();
    } finally {
      if (live()) pending.value = false;
    }
  }

  function enter(owner: string, category: string | null) {
    sequence++;
    pending.value = false;
    error.value = '';
    restart.value = false;
    failedRefresh = false;
    if (epoch !== owner) {
      epoch = owner;
      tagSequence++;
      tagsPending.value = false;
      tagsError.value = '';
      tags.value = undefined;
      saved.clear();
      openers.clear();
    }
    active = category ?? '';
    page.value = saved.get(active);
    if (!active || !epoch) return;
    if (page.value) {
      saved.delete(active);
      saved.set(active, page.value);
    } else void load();
    if (!tagsError.value) void loadTags();
  }

  return {
    tags,
    tagsPending,
    tagsError,
    page,
    pending,
    error,
    nextBatch,
    enter,
    load,
    loadTags,
    retry: () => load(restart.value || failedRefresh),
    remember: (id: string) => {
      if (active) openers.set(active, id);
    },
    takeOpener() {
      const id = openers.get(active);
      openers.delete(active);
      return id;
    },
  };
}
