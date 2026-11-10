import { computed, onScopeDispose, ref, shallowRef, watch, type Ref } from 'vue';
import {
  failure,
  type AccountView,
  type CoreClient,
  type TrackPage,
} from '../services/contracts.ts';
import {
  nextPlaylistDurationOrder,
  playlistDurationSource,
  type PlaylistDurationOrder,
  type PlaylistDurationPage,
  type PlaylistDurationScope,
} from '../services/playlistDuration.ts';
import type { Page } from './useMusic';

export function usePlaylistDuration(
  core: Pick<CoreClient, 'playlistDurationPrepare'>,
  account: () => AccountView,
  route: () => Page,
  shown: Ref<TrackPage | undefined>,
  loading: () => boolean,
) {
  // M/195 retains the pathname's choice during the process. A new account
  // starts clean; tabs within a playlist share the same header choice.
  const choices = new Map<string, PlaylistDurationOrder>();
  const order = ref<PlaylistDurationOrder>('original');
  const result = shallowRef<PlaylistDurationPage>();
  const pending = ref(false);
  const issue = ref('');
  const delayedNotice = ref(false);
  let sequence = 0;
  let activeKey = '';
  let owner = '';
  let lastRoute: Page | undefined;
  let lastSource = '';
  let lastShown: TrackPage | undefined;
  let lastLoading = false;
  const source = computed(() => {
    const page = route();
    return page.kind === 'playlist' &&
      !page.zonePlaylistReceipt &&
      account().status === 'authenticated'
      ? playlistDurationSource(page.id, shown.value)
      : undefined;
  });
  const scope = computed<PlaylistDurationScope | undefined>(() => {
    if (order.value !== 'original' && result.value && source.value && !loading())
      return { receipt: result.value.receipt, order: order.value };
  });
  const ordinals = computed(() =>
    order.value === 'original' || !scope.value ? undefined : result.value?.[order.value],
  );
  const requested = computed(() => order.value !== 'original' && !!source.value);

  function clear() {
    ++sequence;
    activeKey = '';
    pending.value = false;
    delayedNotice.value = false;
    issue.value = '';
    result.value = undefined;
  }
  async function prepare() {
    const target = route();
    const epoch = account().epoch;
    const captured = source.value;
    if (
      !captured ||
      target.kind !== 'playlist' ||
      loading() ||
      order.value === 'original' ||
      result.value ||
      pending.value
    )
      return;
    const key = JSON.stringify(captured);
    const serial = ++sequence;
    activeKey = key;
    const current = () =>
      serial === sequence &&
      activeKey === key &&
      route() === target &&
      account().epoch === epoch &&
      !loading() &&
      JSON.stringify(source.value) === key &&
      order.value !== 'original';
    issue.value = '';
    pending.value = true;
    const timer = setTimeout(() => {
      if (current()) delayedNotice.value = true;
    }, 1_000);
    try {
      const response = await core.playlistDurationPrepare(epoch, target.id, captured);
      if (!current() || response.epoch !== epoch) return;
      const value = response.value;
      if (
        value.page.playlist?.id !== target.id ||
        JSON.stringify(playlistDurationSource(target.id, value.page)) !== key ||
        value.page.more ||
        value.page.tracks.length !== value.page.total
      )
        throw { code: 'snapshot_changed', message: '歌单已更新，请刷新后重试' };
      result.value = value;
      shown.value = value.page;
    } catch (error) {
      if (current()) issue.value = failure(error).message;
    } finally {
      clearTimeout(timer);
      if (serial === sequence) {
        pending.value = false;
        delayedNotice.value = false;
      }
    }
  }
  watch(
    [route, () => account().epoch, () => account().status, source, () => shown.value, loading],
    ([target, epoch, status, nextSource, nextShown, isLoading]) => {
      const accountChanged = owner !== epoch || status !== 'authenticated';
      if (accountChanged) {
        choices.clear();
        owner = epoch;
      }
      const key = JSON.stringify(nextSource) ?? '';
      const externalRead = nextShown !== lastShown && nextShown !== result.value?.page;
      const refreshing = isLoading && !lastLoading;
      lastLoading = isLoading;
      lastShown = nextShown;
      if (
        accountChanged ||
        target !== lastRoute ||
        key !== lastSource ||
        externalRead ||
        refreshing
      ) {
        clear();
        lastRoute = target;
        lastSource = key;
        order.value =
          target.kind === 'playlist' ? (choices.get(target.id) ?? 'original') : 'original';
      }
      if (!issue.value) void prepare();
    },
    { immediate: true, flush: 'sync' },
  );
  onScopeDispose(clear);
  function cycle() {
    const target = route();
    if (target.kind !== 'playlist' || !source.value) return;
    order.value = nextPlaylistDurationOrder(order.value);
    choices.delete(target.id);
    if (order.value !== 'original') {
      choices.set(target.id, order.value);
      if (choices.size > 64) choices.delete(choices.keys().next().value!);
      void prepare();
    } else clear();
  }
  return {
    order,
    scope,
    ordinals,
    requested,
    supported: computed(() => !!source.value),
    blocked: computed(() => requested.value && !scope.value),
    pending,
    delayedNotice,
    issue,
    cycle,
    retry: prepare,
  };
}
