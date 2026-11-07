import { computed, onScopeDispose, shallowRef, watch } from 'vue';
import { failure } from '../services/contracts.ts';
import type {
  VoiceSongGroupDownloadClient,
  VoiceSongGroupDownloadPlan,
  VoiceSongGroupDownloadRequest,
  VoiceSongGroupDownloadTarget,
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
    hints.every((hint) => hint === 'ordinary' || hint === 'omitted') && hints.includes('ordinary')
  );
}

export type VoiceSongGroupDownloadNotice =
  | { kind: 'busy' | 'queued' | 'blocked'; text: string }
  | { kind: 'directory' | 'uncertain'; text: string };

export interface VoiceSongGroupDownloadOpening {
  request: VoiceSongGroupDownloadRequest;
  live: () => boolean;
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
  Exclude<VoiceSongGroupDownloadPlan['decision'], 'ready' | 'directory_required'>,
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
  const notice = computed(() => (opening.value?.live() ? state.value : undefined));
  const busy = computed(() => notice.value?.kind === 'busy');

  function retire() {
    opening.value = undefined;
    state.value = undefined;
  }
  watch(
    () => opening.value?.live(),
    (live) => {
      if (live === false) retire();
    },
    { flush: 'sync' },
  );
  onScopeDispose(retire);

  const owns = (intent: VoiceSongGroupDownloadOpening) => opening.value === intent && intent.live();
  const uncertain: VoiceSongGroupDownloadNotice = {
    kind: 'uncertain',
    text: '未能确认下载任务是否添加，请在下载管理中核对。',
  };

  async function run(opened: VoiceSongGroupDownloadOpening) {
    if (!opened.live() || busy.value) return;
    const intent = {
      request: { ...opened.request, indices: [...opened.request.indices] },
      live: opened.live,
    };
    opening.value = intent;
    state.value = { kind: 'busy', text: '正在检查所选歌曲…' };
    const { epoch, playId, cardReceipt } = intent.request;
    let attempted = false;
    try {
      const plan = await core.playerVoiceSongsDownloadPrepare(intent.request);
      const matches = planMatches(plan, intent.request);
      const target: VoiceSongGroupDownloadTarget | undefined =
        matches && plan.decision === 'ready' && plan.selectionReceipt
          ? { epoch, playId, cardReceipt, selectionReceipt: plan.selectionReceipt }
          : undefined;
      if (!owns(intent)) {
        // The discarded plan may have reached the native shelf before the
        // overlay closed. Retire only that exact receipt and never start it.
        if (target) void core.playerVoiceSongsDownloadCancel(target).catch(() => {});
        return;
      }
      if (!matches || (plan.decision === 'ready' && (!target || !plan.eligibleIndices.length))) {
        state.value = { kind: 'blocked', text: '所选歌曲已变化，请重新选择。' };
        return;
      }
      if (plan.decision === 'directory_required') {
        state.value = { kind: 'directory', text: '尚未设置可用的下载目录。' };
        return;
      }
      if (plan.decision !== 'ready' || !target) {
        if (plan.decision !== 'ready')
          state.value = { kind: 'blocked', text: blocked[plan.decision] };
        return;
      }
      state.value = { kind: 'busy', text: '正在添加到下载列表…' };
      attempted = true;
      const result = await core.playerVoiceSongsDownloadStart(target);
      if (!owns(intent)) return;
      if (
        result.epoch !== epoch ||
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
    } catch (error) {
      if (!owns(intent)) return;
      const issue = failure(error);
      if (issue.code === 'session_expired' || issue.code === 'profile_changed') expired();
      if (!owns(intent)) return;
      state.value = attempted ? uncertain : { kind: 'blocked', text: issue.message };
    }
  }

  function action() {
    const intent = opening.value;
    if (!intent || !owns(intent)) return;
    if (notice.value?.kind === 'directory') settings(intent.request.epoch);
    else if (notice.value?.kind === 'uncertain') downloads(intent.request.epoch);
  }

  return { notice, busy, run, action };
}
