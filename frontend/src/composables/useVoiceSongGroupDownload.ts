import { computed, onScopeDispose, shallowRef, watch } from 'vue';
import type { DownloadResult } from '../services/downloads.ts';
import { failure } from '../services/contracts.ts';
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
  liked: ReadonlySet<string>,
  busyLikes: ReadonlySet<string>,
) {
  const row = card?.tracks[anchor];
  if (
    !card?.receipt ||
    !row ||
    indices.length < 2 ||
    !indices.includes(anchor) ||
    !liked.has(row.id) ||
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
  | { kind: 'directory' | 'uncertain'; text: string };

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
  playlist_required: '触发这次下载的歌曲未确认喜欢，请使用单曲下载。',
  membership_unknown: '尚未核实歌曲的喜欢状态，请稍后再试。',
  unavailable: '所选歌曲没有可新建的下载任务。',
  unsupported_selection: '所选歌曲包含暂不支持多选下载的曲目，请使用单曲下载。',
} satisfies Record<
  Exclude<
    VoiceSongGroupDownloadPlan['decision'],
    'ready' | 'directory_required' | 'purchased_quality_required'
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
  const notice = computed(() => (opening.value?.live() ? state.value : undefined));
  const quality = computed(() =>
    prepared.value && owns(prepared.value.opening) ? prepared.value : undefined,
  );
  const busy = computed(() => notice.value?.kind === 'busy');

  function cancel(target: VoiceSongGroupDownloadTarget) {
    void core.playerVoiceSongsDownloadCancel(target).catch(() => {});
  }
  function retire() {
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
      text: `已添加到下载列表（${result.accepted.length} 首${suffix}）`,
    };
  }

  async function run(opened: VoiceSongGroupDownloadOpening) {
    if (!opened.live() || busy.value || quality.value) return;
    const intent = {
      ...opened,
      request: { ...opened.request, indices: [...opened.request.indices] },
    };
    opening.value = intent;
    state.value = { kind: 'busy', text: '正在检查所选歌曲…' };
    const { epoch, playId, cardReceipt } = intent.request;
    let attempted = false;
    try {
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
        ((plan.decision === 'ready' || plan.decision === 'purchased_quality_required') &&
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

  function action() {
    const intent = opening.value;
    if (!intent || !owns(intent)) return;
    if (notice.value?.kind === 'directory') settings(intent.request.epoch);
    else if (notice.value?.kind === 'uncertain') downloads(intent.request.epoch);
  }

  return { notice, busy, quality, run, action, closeQuality, confirmQuality, openQualityGuide };
}
