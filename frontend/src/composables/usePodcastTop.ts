import { computed, reactive, ref, watch } from 'vue';
import { failure, type AccountView } from '../services/contracts';
import { useCore } from '../services/core';
import type { PodcastRankRow, PodcastRankTarget, PodcastTopLayout } from '../services/podcast';

const TARGETS = {
  score: { group: 'audio', id: 'score', chartType: '7' },
  hot: { group: 'voice', id: 'daily', chartType: '3' },
  recommended: { group: 'overall_charts', id: 'RECOMMEND_CHARTS', chartType: '0' },
  free: { group: 'free_charts', id: 'VOICE_BOOK_FREE_CHARTS_ALL', chartType: '0' },
} as const satisfies Record<string, PodcastRankTarget>;
export type PodcastPreview = keyof typeof TARGETS;
interface Preview {
  pending?: boolean;
  rows?: PodcastRankRow[];
  error?: string;
}

export function usePodcastTop(
  account: () => AccountView,
  personalized: () => boolean | undefined,
  expired: () => void,
) {
  const core = useCore();
  const layout = ref<PodcastTopLayout>();
  const previews = reactive<Partial<Record<PodcastPreview, Preview>>>({});
  const ordinary = ref<'score' | 'hot'>('score');
  const bookChoice = ref<'recommended' | 'free'>();
  const canRecommend = computed(() => personalized() === true);
  const book = computed(() => (canRecommend.value ? (bookChoice.value ?? 'recommended') : 'free'));
  let configPending = false,
    generation = 0;
  let active = false;
  const requests = new Map<PodcastPreview, symbol>();
  watch(
    () => [account().epoch, account().status],
    () => {
      generation++;
      configPending = false;
      layout.value = undefined;
      ordinary.value = 'score';
      bookChoice.value = undefined;
      requests.clear();
      for (const key of Object.keys(TARGETS) as PodcastPreview[]) delete previews[key];
    },
    { flush: 'sync' },
  );
  watch(
    book,
    () => {
      if (!canRecommend.value) {
        requests.delete('recommended');
        delete previews.recommended;
      }
      if (active && layout.value === 'updated') void read(book.value);
    },
    { flush: 'sync' },
  );
  watch(
    canRecommend,
    (enabled) => {
      if (!enabled) {
        requests.delete('recommended');
        delete previews.recommended;
      }
    },
    { flush: 'sync' },
  );
  async function read(key: PodcastPreview, retry = false) {
    const { epoch, status } = account();
    if (
      status !== 'authenticated' ||
      (key === 'recommended' && !canRecommend.value) ||
      previews[key]?.pending ||
      (!retry && (previews[key]?.rows || previews[key]?.error))
    )
      return;
    const owner = generation;
    const id = Symbol(key);
    requests.set(key, id);
    const live = () => owner === generation && requests.get(key) === id;
    previews[key] = { pending: true };
    try {
      const result = await core.podcastRankings(epoch, {
        kind: 'entries',
        preview: true,
        ...TARGETS[key],
      });
      if (live() && result.epoch === epoch && result.value.kind === 'entries')
        previews[key] = { rows: result.value.rows };
    } catch (value) {
      if (!live()) return;
      const issue = failure(value);
      previews[key] = { error: issue.message };
      if (issue.code === 'session_expired' || issue.code === 'profile_changed') expired();
    } finally {
      if (live() && previews[key]?.pending) previews[key] = {};
    }
  }
  function select(key: PodcastPreview) {
    if (key === 'score' || key === 'hot') ordinary.value = key;
    else if (key !== 'recommended' || canRecommend.value) bookChoice.value = key;
    void read(key);
  }
  async function activate() {
    active = true;
    const { epoch, status } = account();
    if (status !== 'authenticated' || configPending) return;
    const owner = generation;
    if (!layout.value) {
      configPending = true;
      try {
        const result = await core.podcastRankings(epoch, { kind: 'top_config' });
        if (owner !== generation || result.epoch !== epoch) return;
        layout.value = result.value.kind === 'top_config' ? result.value.layout : 'classic';
      } catch (value) {
        if (owner !== generation) return;
        const issue = failure(value);
        if (issue.code === 'session_expired' || issue.code === 'profile_changed') {
          expired();
          return;
        }
        layout.value = 'classic';
      } finally {
        if (owner === generation) configPending = false;
      }
    }
    if (owner !== generation || !active) return;
    void read(ordinary.value);
    if (layout.value === 'updated') void read(book.value);
  }
  return {
    layout,
    previews,
    ordinary,
    book,
    canRecommend,
    targets: TARGETS,
    read,
    select,
    activate,
    hide: () => {
      active = false;
    },
  };
}
export type PodcastTopModel = ReturnType<typeof usePodcastTop>;
