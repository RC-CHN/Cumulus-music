import { computed, ref, shallowRef, watch } from 'vue';
import { failure, type AccountView } from '../services/contracts';
import { useCore } from '../services/core';
import type {
  PodcastRankGroup,
  PodcastRankMenu,
  PodcastRankRow,
  PodcastRankTab,
  PodcastRankTarget,
} from '../services/podcast';

const ORDINARY: { group: PodcastRankGroup; name: string }[] = [
  { group: 'voice', name: '声音榜' },
  { group: 'audio', name: '播客榜' },
  { group: 'talent', name: '主播榜' },
];
const tabKey = (tab?: PodcastRankTab) => (tab ? `${tab.id}/${tab.chartType}` : '');

export function usePodcastRankings(account: () => AccountView, expired: () => void) {
  const core = useCore();
  const group = ref<PodcastRankGroup>('voice');
  const bookGroups = shallowRef<PodcastRankMenu[]>([]);
  const groups = computed(() => [...ORDINARY, ...bookGroups.value]);
  const menu = shallowRef<PodcastRankTab[]>([]);
  const tab = shallowRef<PodcastRankTab>();
  const rows = shallowRef<PodcastRankRow[]>([]);
  const pending = ref(false),
    error = ref(''),
    configError = ref('');
  const configPending = ref(false),
    configReady = ref(false);
  const limit = ref(20);
  let version = 0,
    request = 0;
  let configRequest = 0;
  let visit: number | undefined;
  let wanted: PodcastRankTarget | undefined;
  const menus = new Map<PodcastRankGroup, PodcastRankTab[]>();
  watch(
    () => [account().epoch, account().status],
    () => {
      version++;
      visit = undefined;
      wanted = undefined;
      request++;
      menus.clear();
      group.value = 'voice';
      bookGroups.value = [];
      menu.value = [];
      tab.value = undefined;
      rows.value = [];
      configPending.value = false;
      configReady.value = false;
      configError.value = '';
      pending.value = false;
      error.value = '';
      limit.value = 20;
    },
    { flush: 'sync' },
  );
  function issue(value: unknown, slot = error) {
    const issue = failure(value);
    slot.value = issue.message;
    if (issue.code === 'session_expired' || issue.code === 'profile_changed') expired();
  }
  async function bookConfig() {
    const { epoch, status } = account();
    if (configPending.value || configReady.value || status !== 'authenticated') return;
    const owner = version;
    const id = ++configRequest;
    const live = () => owner === version && id === configRequest;
    configPending.value = true;
    configError.value = '';
    try {
      const result = await core.podcastRankings(epoch, { kind: 'book_groups' });
      if (live() && result.epoch === epoch && result.value.kind === 'book_groups') {
        bookGroups.value = result.value.groups;
        configReady.value = true;
        const current = bookGroups.value.find((item) => item.group === group.value);
        if (current) menu.value = current.tabs;
      }
    } catch (value) {
      if (live()) issue(value, configError);
    } finally {
      if (live()) configPending.value = false;
    }
  }
  async function open(next: PodcastRankGroup, force = false, target?: PodcastRankTarget) {
    const { epoch, status } = account();
    if (
      status !== 'authenticated' ||
      (!force && next === group.value && (menu.value.length || pending.value))
    )
      return;
    const id = ++request;
    group.value = next;
    menu.value = [];
    tab.value = undefined;
    rows.value = [];
    error.value = '';
    pending.value = true;
    try {
      const cached = menus.get(next) ?? bookGroups.value.find((item) => item.group === next)?.tabs;
      if (cached) menu.value = cached;
      else {
        const result = await core.podcastRankings(epoch, { kind: 'menu', group: next });
        if (id !== request || result.epoch !== epoch || result.value.kind !== 'menu') return;
        menu.value = result.value.tabs;
        menus.set(next, menu.value);
      }
    } catch (value) {
      if (id === request) issue(value);
    } finally {
      if (id === request) pending.value = false;
    }
    if (id === request && menu.value[0]) {
      const matched =
        target &&
        menu.value.find((item) => item.id === target.id && item.chartType === target.chartType);
      await choose(matched || menu.value[0]);
    }
  }
  async function choose(target: PodcastRankTab, force = false) {
    const { epoch, status } = account();
    if (
      status !== 'authenticated' ||
      !menu.value.includes(target) ||
      (!force && tabKey(tab.value) === tabKey(target))
    )
      return;
    const id = ++request;
    tab.value = target;
    rows.value = [];
    limit.value = 20;
    error.value = '';
    pending.value = true;
    try {
      const result = await core.podcastRankings(epoch, {
        kind: 'entries',
        group: group.value,
        id: target.id,
        chartType: target.chartType,
      });
      if (id === request && result.epoch === epoch && result.value.kind === 'entries')
        rows.value = result.value.rows;
    } catch (value) {
      if (id === request) issue(value);
    } finally {
      if (id === request) pending.value = false;
    }
  }
  async function activate(routeVisit = 0, target?: PodcastRankTarget) {
    const fresh = visit !== routeVisit;
    if (fresh) {
      visit = routeVisit;
      wanted = target;
    }
    if (!fresh && (menu.value.length || error.value || pending.value)) {
      void bookConfig();
      return;
    }
    if (target && !ORDINARY.some((item) => item.group === target.group)) {
      await bookConfig();
      if (visit !== routeVisit) return;
      const matching = bookGroups.value.some((item) => item.group === target.group);
      void open(matching ? target.group : 'voice', true, matching ? target : undefined);
    } else {
      void bookConfig();
      void open(target?.group ?? 'voice', true, target);
    }
  }
  function dropPersonalized() {
    // Recheck the preference before exposing the optional tab on next entry.
    configRequest++;
    configPending.value = false;
    configReady.value = false;
    bookGroups.value = [];
    if (tab.value?.id === 'RECOMMEND_CHARTS') {
      request++;
      group.value = 'voice';
      menu.value = [];
      tab.value = undefined;
      rows.value = [];
      pending.value = false;
      error.value = '';
    } else menu.value = menu.value.filter((item) => item.id !== 'RECOMMEND_CHARTS');
  }
  function retry() {
    const current = tab.value && menu.value.find((item) => tabKey(item) === tabKey(tab.value));
    if (current) void choose(current, true);
    else void open(group.value, true, wanted?.group === group.value ? wanted : undefined);
  }
  return {
    groups,
    group,
    menu,
    tab,
    rows,
    limit,
    pending,
    error,
    configError,
    activate,
    bookConfig,
    dropPersonalized,
    open,
    choose,
    retry,
    tabKey,
  };
}
export type PodcastRankingsModel = ReturnType<typeof usePodcastRankings>;
