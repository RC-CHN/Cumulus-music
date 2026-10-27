import { ref, shallowRef } from 'vue';
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
} from '../services/podcast.ts';

export function usePodcasts(core: PodcastClient = useCore(), expired: () => void = () => {}) {
  const page = shallowRef<PodcastPage>();
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
    visible = false;
  let history: PodcastQuery[] = [];
  const canBack = ref(false);
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
    epoch.value = '';
    page.value = undefined;
    liked.value = new Set();
    likesKnown.value = false;
    operations.value = [];
    loading.value = false;
    busy.value = false;
    history = [];
    canBack.value = false;
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
    if (!epoch.value || busy.value) return;
    const g = generation,
      e = epoch.value,
      n = ++read;
    loading.value = true;
    error.value = '';
    try {
      const value = await core.podcastPage(
        e,
        { ...query.value, revision: refresh ? null : query.value.revision },
        refresh,
      );
      if (valid(g, e) && n === read && value.epoch === e) {
        stateRead++;
        page.value = value;
        liked.value = new Set(value.likedIds);
        likesKnown.value = true;
        operations.value = value.operations;
        query.value = { ...query.value, revision: value.revision };
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
  function navigate(scope: PodcastScope) {
    if (scope.kind === 'radio' && query.value.scope.kind !== 'radio') {
      history.push({ ...query.value });
      canBack.value = true;
    }
    if (scope.kind !== 'radio') {
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
    if (value && epoch.value) void load(!page.value);
  }
  function search(keyword: string) {
    query.value = { ...query.value, keyword, offset: 0 };
    return load();
  }
  function paginate(offset: number) {
    query.value = { ...query.value, offset };
    return load();
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
    connect,
    dispose,
    show,
    load,
    state,
    navigate,
    open,
    back,
    search,
    paginate,
    pending,
    command,
    change,
  };
}
export type PodcastModel = ReturnType<typeof usePodcasts>;
