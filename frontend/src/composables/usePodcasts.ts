import { computed, ref, shallowRef } from 'vue';
import type { AccountView } from '../services/contracts.ts';
import { failure } from '../services/contracts.ts';
import { useCore } from '../services/core.ts';
import { podcastReference } from '../services/podcast.ts';
import type {
  PodcastClient,
  PodcastScope,
  PodcastPage,
  PodcastQuery,
  PodcastCommand,
  PodcastAction,
  PodcastMutation,
  PodcastSubscriptionRecommendations,
} from '../services/podcast.ts';

export function usePodcasts(core: PodcastClient = useCore(), expired: () => void = () => {}) {
  const page = shallowRef<PodcastPage>();
  const recommendations = shallowRef<PodcastSubscriptionRecommendations>();
  const query = ref<PodcastQuery>({
    scope: { kind: 'podcasts' },
    keyword: '',
    offset: 0,
    limit: 50,
    revision: null,
  });
  const liked = shallowRef(new Set<string>()),
    likesKnown = ref(false);
  const operations = shallowRef<PodcastMutation[]>([]);
  const epoch = ref(''),
    loading = ref(false),
    busy = ref(false),
    error = ref(''),
    notice = ref('');
  let generation = 0,
    read = 0,
    write = 0,
    stateRead = 0,
    recommendationVisit = 0,
    recommendationsRequested = false,
    visible = false;
  let history: PodcastQuery[] = [];
  const canBack = ref(false);
  const recentSelected = ref(false);
  const recentReturn = ref(false);
  const recommendedCards = computed(() => {
    const snapshot = page.value;
    return !recentSelected.value &&
      query.value.scope.kind === 'podcasts' &&
      snapshot?.scope.kind === 'podcasts' &&
      snapshot.epoch === epoch.value &&
      snapshot.remoteTotal < 50
      ? (recommendations.value?.cards ?? [])
      : [];
  });
  function valid(g: number, e: string) {
    return generation === g && epoch.value === e && !!e;
  }
  function issue(value: unknown) {
    const f = failure(value);
    error.value = f.message;
    if (f.code === 'session_expired' || f.code === 'profile_changed') {
      dispose();
      expired();
    }
  }
  function dispose() {
    generation++;
    read++;
    write++;
    stateRead++;
    recommendationVisit++;
    recommendationsRequested = false;
    recommendations.value = undefined;
    epoch.value = '';
    page.value = undefined;
    liked.value = new Set();
    likesKnown.value = false;
    operations.value = [];
    loading.value = false;
    busy.value = false;
    history = [];
    canBack.value = false;
    recentSelected.value = false;
    recentReturn.value = false;
    query.value = {
      ...query.value,
      scope: { kind: 'podcasts' },
      keyword: '',
      revision: null,
      offset: 0,
    };
  }
  function connect(account: AccountView) {
    dispose();
    error.value = '';
    notice.value = '';
    if (account.status !== 'authenticated') return;
    epoch.value = account.epoch;
    void state(false);
    if (visible) void load(true);
  }
  function activateRecommendations() {
    if (
      !visible ||
      !epoch.value ||
      recommendationsRequested ||
      (!recentSelected.value && query.value.scope.kind === 'radio')
    )
      return;
    recommendationsRequested = true;
    void loadRecommendations();
  }
  async function loadRecommendations() {
    const g = generation,
      e = epoch.value,
      visit = ++recommendationVisit;
    try {
      const result = await core.podcastSubscriptionRecommendations(e);
      if (visible && valid(g, e) && visit === recommendationVisit && result.epoch === e)
        recommendations.value = result.value;
    } catch {
      // This optional shelf never masks the collection or retries within a visit.
    }
  }
  // Disk-only reads expose unresolved writes even while the service is offline.
  async function state(refresh = false) {
    if (!epoch.value) return;
    const g = generation,
      e = epoch.value,
      n = ++stateRead;
    try {
      const result = await core.podcastState(e, refresh);
      if (!valid(g, e) || n !== stateRead || result.epoch !== e) return;
      operations.value = result.value.operations;
      likesKnown.value = result.value.likedIds !== null;
      liked.value = new Set(result.value.likedIds ?? []);
    } catch (value) {
      if (valid(g, e) && n === stateRead) issue(value);
    }
  }
  async function load(refresh = false) {
    activateRecommendations();
    if (!epoch.value || busy.value || recentSelected.value) return;
    const g = generation,
      e = epoch.value,
      n = ++read;
    loading.value = true;
    error.value = '';
    try {
      let request = { ...query.value, revision: refresh ? null : query.value.revision };
      if (request.scope.kind === 'radio' && request.revision === null) {
        const preference = await core.podcastRadioOrder(e, { kind: 'read', id: request.scope.id });
        if (!valid(g, e) || n !== read || preference.epoch !== e) return;
        if (preference.value !== null)
          request = { ...request, scope: { ...request.scope, ascending: preference.value } };
      }
      const value = await core.podcastPage(e, request, refresh);
      if (valid(g, e) && n === read && value.epoch === e) {
        stateRead++;
        page.value = value;
        liked.value = new Set(value.likedIds);
        likesKnown.value = true;
        operations.value = value.operations;
        query.value = { ...request, revision: value.revision };
      }
    } catch (value) {
      if (valid(g, e) && n === read) {
        issue(value);
        await state(false);
      }
    } finally {
      if (valid(g, e) && n === read) loading.value = false;
    }
  }
  function showRecent() {
    if (!epoch.value) return;
    read++;
    history = [];
    canBack.value = false;
    loading.value = false;
    notice.value = '';
    error.value = '';
    recentReturn.value = false;
    recentSelected.value = true;
    activateRecommendations();
  }
  function navigate(scope: PodcastScope, libraryBack = true) {
    if (recentSelected.value) {
      recentReturn.value = libraryBack && scope.kind === 'radio';
      recentSelected.value = false;
    }
    if (!libraryBack || scope.kind !== 'radio') recentReturn.value = false;
    if (recentReturn.value) canBack.value = true;
    else if (libraryBack && scope.kind === 'radio' && query.value.scope.kind !== 'radio') {
      history.push({ ...query.value });
      canBack.value = true;
    }
    if (!libraryBack || scope.kind !== 'radio') {
      history = [];
      canBack.value = false;
    }
    read++;
    page.value = undefined;
    loading.value = false;
    query.value = { ...query.value, scope, keyword: '', offset: 0, revision: null };
    notice.value = '';
    error.value = '';
    if (visible) void load(true);
  }
  function openCard(id: string) {
    const shown = page.value;
    const receipt =
      visible &&
      !recentSelected.value &&
      !loading.value &&
      !busy.value &&
      query.value.scope.kind === 'podcasts' &&
      shown?.scope.kind === 'podcasts' &&
      shown.epoch === epoch.value &&
      shown.cards.some((card) => card.id === id && (card.newEpisodeCount ?? 0) > 0)
        ? shown.subscribedBadgeReceipt
        : undefined;
    const owner = epoch.value;
    navigate({ kind: 'radio', id, ascending: true });
    // Only an explicit subscribed card navigation can submit this mutation.
    // Unavailable/ambiguous clear never prevents ordinary detail navigation.
    if (receipt) void core.podcastClearSubscribedBadge(owner, receipt, id).catch(() => {});
  }
  function recommendedSelection(id: string) {
    const shelf = recommendations.value;
    const displayed = page.value;
    if (
      !visible ||
      busy.value ||
      !shelf ||
      !displayed ||
      !recommendedCards.value.some((card) => card.id === id && !card.unavailable)
    )
      return;
    return { id, receipt: shelf.receipt, revision: displayed.revision };
  }
  function openRecommended(id: string) {
    if (!recommendedSelection(id)) return;
    // This action came from the recommendation shelf, not from a subscribed card.
    navigate({ kind: 'radio', id, ascending: true });
  }
  function canCollectRecommendation(id: string) {
    const displayed = page.value;
    return !!(
      recommendedSelection(id) &&
      displayed &&
      !query.value.keyword.trim() &&
      displayed.offset === 0 &&
      displayed.cards.length === displayed.remoteTotal &&
      !displayed.cards.some((card) => card.id === id) &&
      recommendedCards.value.some((card) => card.id === id && card.ownedByOther === true) &&
      !pending('subscribe', id)
    );
  }
  function collectRecommended(id: string, revision: string) {
    const source = recommendedSelection(id);
    if (source?.revision !== revision || !canCollectRecommendation(id)) return;
    void command({ kind: 'collect_recommendation', ...source });
  }
  function open(reference: string) {
    if (busy.value) return;
    const id = podcastReference(reference);
    if (!id) {
      error.value = '请输入有效的播客编号或网易云播客链接';
      return;
    }
    navigate({ kind: 'radio', id, ascending: true });
  }
  function back() {
    if (recentReturn.value) {
      showRecent();
      return;
    }
    const previous = history.pop();
    if (!previous) return;
    read++;
    page.value = undefined;
    query.value = { ...previous, revision: null };
    canBack.value = history.length > 0;
    if (visible) void load(true);
  }
  function show(value: boolean) {
    visible = value;
    if (!value) {
      recommendationVisit++;
      recommendationsRequested = false;
    } else if (epoch.value) {
      void load(!page.value);
    }
  }
  function search(keyword: string) {
    query.value = { ...query.value, keyword, offset: 0 };
    return load();
  }
  function paginate(offset: number) {
    query.value = { ...query.value, offset };
    return load();
  }
  async function toggleRadioOrder() {
    const scope = query.value.scope;
    const shown = page.value;
    if (
      !epoch.value ||
      busy.value ||
      loading.value ||
      scope.kind !== 'radio' ||
      shown?.scope.kind !== 'radio' ||
      shown.epoch !== epoch.value ||
      shown.scope.id !== scope.id ||
      shown.scope.ascending !== scope.ascending ||
      shown.radio?.id !== scope.id ||
      shown.radio.unavailable
    )
      return;
    const g = generation,
      e = epoch.value,
      n = ++write,
      before = ++read;
    busy.value = true;
    error.value = '';
    let applied = false;
    try {
      const result = await core.podcastRadioOrder(e, {
        kind: 'set',
        scope: shown.scope,
        revision: shown.revision,
        ascending: !shown.scope.ascending,
      });
      if (!valid(g, e) || n !== write || result.epoch !== e) return;
      if (result.value !== !shown.scope.ascending)
        throw { code: 'stale', message: '播客排序未保存', retryable: false };
      applied = true;
      // Keep the old displayed list bound to its order until the replacement
      // page arrives. The next read must resolve the committed preference.
      if (before === read) query.value = { ...query.value, revision: null };
    } catch (value) {
      if (valid(g, e) && n === write && before === read) issue(value);
    } finally {
      if (valid(g, e) && n === write) {
        busy.value = false;
        // Navigation may have waited behind the explicit disk write.
        if (visible && (applied || before !== read)) await load(true);
      }
    }
  }
  function pending(action: PodcastAction, id: string) {
    return operations.value.some((v) => v.action === action && v.id === id);
  }
  async function command(value: PodcastCommand) {
    if (!epoch.value || busy.value || (value.kind === 'change' && pending(value.action, value.id)))
      return;
    const g = generation,
      e = epoch.value,
      n = ++write,
      scope = JSON.stringify(query.value.scope);
    busy.value = true;
    error.value = '';
    notice.value = '';
    read++;
    stateRead++;
    loading.value = false;
    try {
      const result = await core.podcastCommand(e, value);
      if (!valid(g, e) || n !== write || result.epoch !== e) return;
      if (JSON.stringify(query.value.scope) === scope)
        notice.value = ['confirmed', 'unchanged'].includes(result.value.phase)
          ? '已保存'
          : '操作结果仍待核对';
    } catch (value) {
      if (valid(g, e) && n === write) issue(value);
    } finally {
      if (valid(g, e) && n === write) {
        busy.value = false;
        const failed = error.value;
        await state(false);
        if (valid(g, e) && n === write) {
          if (visible) await load(true);
          else await state(true);
          if (valid(g, e) && n === write && failed) error.value = failed;
        }
      }
    }
  }
  async function externalLikeChanged() {
    read++;
    stateRead++;
    loading.value = false;
    if (visible && !busy.value) await load(true);
    else await state(true);
  }
  function change(action: PodcastAction, id: string, desired: boolean) {
    return command({ kind: 'change', action, id, desired });
  }
  return {
    page,
    query,
    liked,
    likesKnown,
    operations,
    epoch,
    loading,
    busy,
    error,
    notice,
    canBack,
    recentSelected,
    recentReturn,
    showRecent,
    connect,
    dispose,
    show,
    load,
    state,
    externalLikeChanged,
    navigate,
    openCard,
    recommendedCards,
    recommendedSelection,
    canCollectRecommendation,
    collectRecommended,
    openRecommended,
    open,
    back,
    search,
    paginate,
    toggleRadioOrder,
    pending,
    command,
    change,
  };
}
export type PodcastModel = ReturnType<typeof usePodcasts>;
