import { computed, nextTick, shallowRef, watch } from 'vue';
import { failure, type CoreClient } from '../services/contracts.ts';
import type { HistoryRowShareChoice } from '../services/history.ts';
import { validMemberQr } from '../services/member.ts';
import type { SearchSongShareClient, SearchSongShareTarget } from '../services/search.ts';

export interface SearchSongShareVisit {
  epoch: string;
  target: SearchSongShareTarget;
  live: () => boolean;
  fallbackFocus: () => HTMLElement | null;
}

type Flow = { visit: SearchSongShareVisit } & (
  | { stage: 'preparing' }
  | { stage: 'chooser'; offer: string }
  | { stage: 'choosing'; offer: string }
  | { stage: 'qr'; modules: boolean[][] }
);

export function useSearchSongShare(
  core: SearchSongShareClient & Pick<CoreClient, 'copyText'>,
  expired: () => void,
) {
  const flow = shallowRef<Flow>();
  const focus = shallowRef<{ visit: SearchSongShareVisit; anchor: HTMLElement }>();
  const feedback = shallowRef<{ text: string; error: boolean }>();
  const dialog = computed(() => {
    const state = flow.value;
    if (state?.stage === 'chooser' || state?.stage === 'qr') return state;
  });
  const restoreFocus = computed(() => focus.value?.visit.live() === true);
  const returnFocus = () => focus.value?.anchor ?? null;
  const fallbackFocus = () => focus.value?.visit.fallbackFocus() ?? null;
  let directCopyAttempt = 0;
  let directCopyPending = false;

  function retire(visit: SearchSongShareVisit, offer: string) {
    void core.searchSongShareCancel(visit.epoch, visit.target, offer).catch(() => {});
  }
  function close(restore = true) {
    const previous = flow.value;
    flow.value = undefined;
    if (!restore || !restoreFocus.value) focus.value = undefined;
    if (previous?.stage === 'chooser' || previous?.stage === 'choosing')
      retire(previous.visit, previous.offer);
  }
  function clearFeedback() {
    directCopyAttempt += 1;
    feedback.value = undefined;
  }
  function reset() {
    close(false);
    clearFeedback();
  }
  function keepFocusVisible() {
    void nextTick(() => {
      const anchor = returnFocus();
      if (restoreFocus.value && anchor?.isConnected && document.activeElement === anchor)
        anchor.scrollIntoView({ block: 'nearest', inline: 'nearest', behavior: 'instant' });
    });
  }
  watch(
    () => focus.value && restoreFocus.value,
    (live) => {
      if (live === false) reset();
    },
    { flush: 'sync' },
  );

  function current(state: Flow) {
    return flow.value === state && state.visit.live();
  }
  function report(text: string, error = true) {
    feedback.value = { text, error };
    keepFocusVisible();
  }
  function denied(kind: 'unavailable' | 'album_required' | 'login_required') {
    report(
      kind === 'album_required' ? '这首歌曲需要购买数字专辑后才能分享。' : '当前资源暂不支持分享。',
    );
  }
  function rejected(cause: unknown) {
    const issue = failure(cause);
    if (issue.code === 'profile_changed' || issue.code === 'session_expired') expired();
    else if (['stale', 'snapshot_changed', 'queue_changed'].includes(issue.code))
      report('搜索结果已更新，请重新打开菜单。');
    else report('分享失败，请重新打开菜单重试。');
  }
  async function open(visit: SearchSongShareVisit, anchor: HTMLElement) {
    if (!visit.live()) return;
    reset();
    focus.value = { visit, anchor };
    const pending = { stage: 'preparing', visit } as const;
    flow.value = pending;
    try {
      const { epoch, value } = await core.searchSongSharePrepare(visit.epoch, visit.target);
      if (epoch !== visit.epoch) {
        if (current(pending)) reset();
        return;
      }
      if (!current(pending)) {
        if (value.kind === 'ready') retire(visit, value.receipt);
        return;
      }
      if (value.kind === 'ready') flow.value = { stage: 'chooser', visit, offer: value.receipt };
      else {
        flow.value = undefined;
        denied(value.kind);
      }
    } catch (cause) {
      if (current(pending)) {
        flow.value = undefined;
        rejected(cause);
      }
    }
  }
  async function choose(choice: HistoryRowShareChoice) {
    const previous = flow.value;
    if (previous?.stage !== 'chooser' || !current(previous)) return;
    const { visit, offer } = previous;
    const pending = { stage: 'choosing', visit, offer } as const;
    flow.value = pending;
    await nextTick();
    if (!current(pending)) return;
    try {
      const { epoch, value } = await core.searchSongShareChoose(
        visit.epoch,
        visit.target,
        offer,
        choice,
      );
      if (!current(pending)) return;
      if (epoch !== visit.epoch) {
        reset();
        return;
      }
      if (value.kind === 'wechat_qr' && choice === 'wechat_qr' && validMemberQr(value.modules)) {
        flow.value = { stage: 'qr', visit, modules: value.modules };
        return;
      }
      if (value.kind === 'copy' && choice === 'copy') {
        try {
          await core.copyText(value.url);
          if (current(pending)) report('复制成功', false);
        } catch {
          if (current(pending)) report('复制失败，请重新打开菜单重试。');
        }
      } else if (value.kind === 'unavailable' || value.kind === 'album_required')
        denied(value.kind);
      else report('暂时无法完成此分享。');
      if (current(pending)) flow.value = undefined;
    } catch (cause) {
      if (current(pending)) {
        flow.value = undefined;
        rejected(cause);
      }
    }
  }

  async function copyPublic(link: string, live: () => boolean) {
    if (!live() || directCopyPending) return;
    clearFeedback();
    const attempt = directCopyAttempt;
    const current = () => attempt === directCopyAttempt && live();
    directCopyPending = true;
    try {
      await core.copyText(link);
      if (current()) report('链接已复制', false);
    } catch {
      if (current()) report('复制链接失败，请重新打开菜单重试。');
    } finally {
      directCopyPending = false;
    }
  }

  return {
    dialog,
    feedback,
    open,
    choose,
    copyPublic,
    close,
    reset,
    clearFeedback,
    returnFocus,
    restoreFocus,
    fallbackFocus,
    keepFocusVisible,
  };
}
