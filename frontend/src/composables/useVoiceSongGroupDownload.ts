import { computed, onScopeDispose, shallowRef, watch } from 'vue';
import type { DownloadResult } from '../services/downloads.ts';
import { failure } from '../services/contracts.ts';
import { useVoiceSongGroupDestination } from './useVoiceSongGroupDestination.ts';
import type {
  VoiceSongGroupDownloadClient,
  VoiceSongGroupDownloadPlan,
  VoiceSongGroupDownloadRequest,
  VoiceSongGroupDownloadTarget,
  VoiceSongPurchasedQuality,
  VoiceSongsView,
} from '../services/voiceSongs.ts';

export function offerVoiceSongGroupDownload(
  card: VoiceSongsView | undefined,
  anchor: number,
  indices: readonly number[],
  authenticated: boolean,
  busyLikes: ReadonlySet<string>,
) {
  const row = card?.tracks[anchor];
  if (
    !card?.receipt ||
    !row ||
    indices.length < 2 ||
    !indices.includes(anchor) ||
    !authenticated ||
    busyLikes.has(row.id)
  )
    return false;
  const hints = indices.map((index) => card?.availability[index]?.groupDownload);
  return (
    hints.every((hint) => hint === 'ordinary' || hint === 'purchased' || hint === 'omitted') &&
    (hints.includes('ordinary') || hints.includes('purchased'))
  );
}

export type VoiceSongGroupDownloadNotice =
  | { kind: 'busy' | 'queued' | 'blocked'; text: string }
  | { kind: 'directory' | 'uncertain' | 'playlist_pending'; text: string };

export interface VoiceSongGroupDownloadOpening {
  request: VoiceSongGroupDownloadRequest;
  live: () => boolean;
  opener?: HTMLElement;
}

type PurchasePlan = Extract<VoiceSongGroupDownloadPlan, { decision: 'purchased_quality_required' }>;

interface PreparedQuality {
  opening: VoiceSongGroupDownloadOpening;
  target: VoiceSongGroupDownloadTarget;
  plan: PurchasePlan;
}

function planMatches(
  plan: VoiceSongGroupDownloadPlan,
  request: VoiceSongGroupDownloadRequest,
): boolean {
  if (
    plan.epoch !== request.epoch ||
    plan.playId !== request.playId ||
    plan.anchorIndex !== request.anchorIndex
  )
    return false;
  // Each projected list is an ordered subsequence and together they must
  // partition the exact frozen selection. Never translate back from song IDs.
  let eligible = 0;
  let omitted = 0;
  for (const index of request.indices) {
    if (plan.eligibleIndices[eligible] === index) eligible++;
    else if (plan.omittedIndices[omitted] === index) omitted++;
    else return false;
  }
  return eligible === plan.eligibleIndices.length && omitted === plan.omittedIndices.length;
}

const blocked = {
  membership_unknown: '尚未核实歌曲的喜欢状态，请稍后再试。',
  unavailable: '所选歌曲没有可新建的下载任务。',
  unsupported_selection: '所选歌曲包含暂不支持多选下载的曲目，请使用单曲下载。',
} satisfies Record<
  Exclude<
    VoiceSongGroupDownloadPlan['decision'],
    'ready' | 'directory_required' | 'purchased_quality_required' | 'playlist_required'
  >,
  string
>;

export function useVoiceSongGroupDownload(
  core: VoiceSongGroupDownloadClient,
  expired: () => void,
  settings: (epoch: string) => void,
  downloads: (epoch: string) => void,
) {
  const opening = shallowRef<VoiceSongGroupDownloadOpening>();
  const state = shallowRef<VoiceSongGroupDownloadNotice>();
  const prepared = shallowRef<PreparedQuality>();
  const owns = (intent: VoiceSongGroupDownloadOpening) => opening.value === intent && intent.live();
  const destination = useVoiceSongGroupDestination(core, expired);
  const notice = computed<VoiceSongGroupDownloadNotice | undefined>(() => {
    if (!opening.value?.live()) return;
    if (!destination.visible.value && destination.recoverable.value)
      return { kind: 'playlist_pending', text: '这次下载的歌单操作待处理。' };
    return state.value;
  });
  const quality = computed(() =>
    prepared.value && owns(prepared.value.opening) ? prepared.value : undefined,
  );
  const busy = computed(() => notice.value?.kind === 'busy');
  const active = computed(() => busy.value || !!quality.value || destination.visible.value);
  const opener = computed(() => opening.value?.opener);

  function cancel(target: VoiceSongGroupDownloadTarget) {
    void core.playerVoiceSongsDownloadCancel(target).catch(() => {});
  }
  function retire() {
    destination.retire(true);
    const chooser = prepared.value;
    const submitted = state.value?.kind === 'busy';
    prepared.value = undefined;
    opening.value = undefined;
    state.value = undefined;
    if (chooser && !submitted) cancel(chooser.target);
  }
  watch(
    () => opening.value?.live(),
    (live) => {
      if (live === false) retire();
    },
    { flush: 'sync' },
  );
  onScopeDispose(retire);

  const uncertain: VoiceSongGroupDownloadNotice = {
    kind: 'uncertain',
    text: '未能确认下载任务是否添加，请在下载管理中核对。',
  };

  function settle(
    result: DownloadResult,
    target: VoiceSongGroupDownloadTarget,
    plan: VoiceSongGroupDownloadPlan,
    addFailed = false,
  ) {
    if (
      result.epoch !== target.epoch ||
      result.operationId !== target.selectionReceipt ||
      result.accepted.length !== plan.eligibleIndices.length ||
      new Set(result.accepted).size !== result.accepted.length ||
      result.failed.length
    ) {
      state.value = uncertain;
      return;
    }
    const suffix = plan.omittedIndices.length ? `，跳过 ${plan.omittedIndices.length} 首` : '';
    state.value = {
      kind: 'queued',
      text: `${addFailed ? '未能收藏到歌单，' : ''}已添加到下载列表（${result.accepted.length} 首${suffix}）`,
    };
  }

  async function run(opened: VoiceSongGroupDownloadOpening) {
    if (!opened.live() || active.value) return;
    if (destination.recoverable.value && opening.value?.live()) {
      destination.reopen();
      return;
    }
    const intent = {
      ...opened,
      request: { ...opened.request, indices: [...opened.request.indices] },
    };
    opening.value = intent;
    state.value = { kind: 'busy', text: '正在检查所选歌曲…' };
    const { epoch, playId, cardReceipt } = intent.request;
    let attempted = false;
    try {
      const previous = destination.drained();
      if (previous) await previous;
      if (!owns(intent)) return;
      const plan = await core.playerVoiceSongsDownloadPrepare(intent.request);
      const matches = planMatches(plan, intent.request);
      const target: VoiceSongGroupDownloadTarget | undefined =
        matches && plan.selectionReceipt
          ? { epoch, playId, cardReceipt, selectionReceipt: plan.selectionReceipt }
          : undefined;
      if (!owns(intent)) {
        // The discarded plan may have reached the native shelf before the
        // overlay closed. Retire only that exact receipt and never start it.
        if (target) cancel(target);
        return;
      }
      if (
        !matches ||
        (['ready', 'purchased_quality_required', 'playlist_required'].includes(plan.decision) &&
          (!target || !plan.eligibleIndices.length))
      ) {
        if (target) cancel(target);
        state.value = { kind: 'blocked', text: '所选歌曲已变化，请重新选择。' };
        return;
      }
      if (plan.decision === 'purchased_quality_required') {
        if (!target) return;
        if (!plan.purchasedQualities.length) {
          cancel(target);
          state.value = { kind: 'blocked', text: '没有可选择的已购音质。' };
          return;
        }
        prepared.value = { opening: intent, target, plan };
        state.value = undefined;
        return;
      }
      if (plan.decision === 'directory_required') {
        state.value = { kind: 'directory', text: '尚未设置可用的下载目录。' };
        return;
      }
      if (plan.decision === 'playlist_required') {
        if (target) openDestination(intent, target, plan);
        return;
      }
      if (plan.decision !== 'ready') {
        state.value = { kind: 'blocked', text: blocked[plan.decision] };
        return;
      }
      if (!target) return;
      state.value = { kind: 'busy', text: '正在添加到下载列表…' };
      attempted = true;
      const result = await core.playerVoiceSongsDownloadStart(target);
      if (!owns(intent)) return;
      settle(result, target, plan);
    } catch (error) {
      if (!owns(intent)) return;
      const issue = failure(error);
      if (issue.code === 'session_expired' || issue.code === 'profile_changed') expired();
      if (!owns(intent)) return;
      state.value = attempted ? uncertain : { kind: 'blocked', text: issue.message };
    }
  }

  function closeQuality() {
    if (quality.value && !busy.value) retire();
  }

  async function openQualityGuide() {
    const chooser = quality.value;
    if (!chooser || busy.value) return;
    await core.openVoiceSongGroupPurchasedQualityGuide(chooser.target);
  }

  async function confirmQuality(tier: VoiceSongPurchasedQuality) {
    const chooser = quality.value;
    if (!chooser || busy.value || !chooser.plan.purchasedQualities.includes(tier)) return;
    const { opening: intent, target, plan } = chooser;
    state.value = { kind: 'busy', text: '正在添加到下载列表…' };
    try {
      const confirmation = await core.playerVoiceSongsDownloadConfirmQuality(target, tier);
      if (!owns(intent)) return;
      prepared.value = undefined;
      if (confirmation.kind === 'started') {
        settle(confirmation.result, target, plan);
        return;
      }
      if (confirmation.kind === 'playlist_required') {
        openDestination(intent, target, plan);
        return;
      }
      // Native explicitly observed no start. Retire the exact receipt;
      // settings or membership changes never retry this confirmation.
      cancel(target);
      state.value =
        confirmation.kind === 'directory_required'
          ? { kind: 'directory', text: '尚未设置可用的下载目录。' }
          : { kind: 'blocked', text: blocked[confirmation.kind] };
    } catch (error) {
      if (!owns(intent)) return;
      const issue = failure(error);
      if (issue.code === 'session_expired' || issue.code === 'profile_changed') expired();
      if (!owns(intent)) return;
      prepared.value = undefined;
      state.value = uncertain;
    }
  }

  function openDestination(
    intent: VoiceSongGroupDownloadOpening,
    target: VoiceSongGroupDownloadTarget,
    plan: VoiceSongGroupDownloadPlan,
  ) {
    state.value = undefined;
    destination.open({
      target,
      live: () => owns(intent),
      retired: () => {
        if (owns(intent)) retire();
      },
      async start(continuation, addFailed) {
        if (!owns(intent)) return;
        state.value = { kind: 'busy', text: '正在添加到下载列表…' };
        try {
          const reply = await core.playerVoiceSongsDownloadDestination(target, {
            kind: 'start',
            continuation,
          });
          if (!owns(intent)) return;
          if (reply.kind === 'started') settle(reply.result, target, plan, addFailed);
          else if (reply.kind === 'directory_required') {
            cancel(target);
            state.value = { kind: 'directory', text: '尚未设置可用的下载目录。' };
          } else state.value = uncertain;
        } catch (error) {
          if (!owns(intent)) return;
          const issue = failure(error);
          if (issue.code === 'session_expired' || issue.code === 'profile_changed') expired();
          if (owns(intent)) state.value = uncertain;
        }
      },
    });
  }

  function action() {
    const intent = opening.value;
    if (!intent || !owns(intent)) return;
    if (notice.value?.kind === 'directory') settings(intent.request.epoch);
    else if (notice.value?.kind === 'uncertain') downloads(intent.request.epoch);
    else if (notice.value?.kind === 'playlist_pending') destination.reopen();
  }

  return {
    notice,
    busy,
    quality,
    active,
    opener,
    destination,
    run,
    action,
    closeQuality,
    confirmQuality,
    openQualityGuide,
  };
}
