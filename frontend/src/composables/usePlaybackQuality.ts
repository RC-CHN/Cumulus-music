import { computed, onScopeDispose, ref, shallowRef, watch } from 'vue';
import { failure } from '../services/contracts.ts';
import type { PlayerModel } from './usePlayer.ts';
import type {
  PlayerClient,
  Quality,
  QualityChoiceView,
  QualityDetailsView,
  QualityTrialView,
} from '../services/player.ts';
import { qualityUnavailable } from '../services/playbackQuality.ts';

export interface QualityContext {
  epoch: string;
  playId: string;
  revision: string;
  songId: string;
  song: boolean;
}
export interface QualityPrompt {
  kind: 'confirm' | 'trial' | 'notice';
  quality: Quality;
  target: QualityContext;
  choice?: QualityChoiceView;
  detailsRevision?: string;
  trial?: QualityTrialView;
  message?: string;
}
type Player = Pick<PlayerModel, 'view' | 'ready' | 'loaded' | 'busy' | 'action' | 'qualityTrial'>;
export function usePlaybackQuality(
  player: Player,
  core: Pick<PlayerClient, 'playerQualityDetails'>,
) {
  const receivedAt = ref(0);
  const opened = ref(false),
    loading = ref(false),
    readError = ref(''),
    dialogBusy = ref(false),
    dialogError = ref('');
  const details = shallowRef<QualityDetailsView>(),
    prompt = shallowRef<QualityPrompt>();
  let alive = true,
    read = 0,
    operation = 0,
    timer: ReturnType<typeof setTimeout> | undefined;
  let captured: QualityContext | undefined;
  const current = computed<QualityContext | undefined>(() => {
    const view = player.view.value;
    if (
      !view?.playId ||
      !view.track ||
      !player.loaded.value ||
      !view.canChangeQuality ||
      view.qualityPreferences.damaged
    )
      return;
    return {
      epoch: view.epoch,
      playId: view.playId,
      revision: view.qualityPreferences.revision ?? 'podcast',
      songId: view.track.id,
      song: view.qualityPreferences.appliesToCurrent,
    };
  });
  const same = (a?: QualityContext, b?: QualityContext) =>
    !!a && !!b && a.epoch === b.epoch && a.playId === b.playId && a.revision === b.revision;
  const targetChanged = computed(() => !!prompt.value && !same(prompt.value.target, current.value));
  const partial = computed(
    () =>
      !!details.value &&
      [
        details.value.status.privilege,
        details.value.status.subscriber,
        details.value.status.sizes,
        ...Object.values(details.value.status.useRights),
      ].some((v) => v !== 'ready'),
  );
  function clearTimer() {
    if (timer !== undefined) clearTimeout(timer);
    timer = undefined;
  }
  function hide() {
    opened.value = false;
    read++;
    loading.value = false;
    clearTimer();
  }
  function show() {
    opened.value = true;
    captured = current.value;
    details.value = undefined;
    readError.value = '';
    void refresh();
  }
  async function fetch(target: QualityContext) {
    const result = await core.playerQualityDetails(target.epoch, target.playId);
    if (
      !same(target, current.value) ||
      result.epoch !== target.epoch ||
      result.playId !== target.playId ||
      result.songId !== target.songId
    )
      throw { code: 'stale', message: '歌曲或音质偏好已变化，请重新打开菜单', retryable: false };
    return result;
  }
  async function refresh() {
    const target = captured;
    if (!opened.value || !target?.song || !same(target, current.value)) return;
    clearTimer();
    const revision = ++read;
    loading.value = true;
    readError.value = '';
    try {
      const result = await fetch(target);
      if (!alive || revision !== read || !opened.value) return;
      details.value = result;
      receivedAt.value = Date.now();
      if (result.refreshAfterMs != null && !partial.value)
        timer = setTimeout(
          () => void refresh(),
          Math.max(1000, Math.min(result.refreshAfterMs, 2147483647)),
        );
    } catch (e) {
      if (alive && revision === read && opened.value) readError.value = failure(e).message;
    } finally {
      if (alive && revision === read) loading.value = false;
    }
  }
  function closePrompt() {
    operation++;
    prompt.value = undefined;
    dialogError.value = '';
    dialogBusy.value = false;
  }
  function choose(quality: Quality) {
    const target = captured;
    if (
      !target ||
      !same(target, current.value) ||
      player.busy.value ||
      dialogBusy.value ||
      !player.ready.value
    )
      return;
    const choice = details.value?.choices.find((c) => c.quality === quality);
    const trial = details.value?.trials.find((t) => t.right === quality);
    if (target.song && trial && ['sending', 'uncertain', 'acknowledged'].includes(trial.phase)) {
      prompt.value = { kind: 'trial', quality, target, choice, trial };
      dialogError.value = '';
      return;
    }
    const view = player.view.value;
    // Like the official selector, selecting the current quality is inert. A
    // downgraded stream is different: explicitly choosing its actual quality
    // must still replace a higher requested/saved preference.
    if (
      view?.actualQuality === quality &&
      view.requestedQuality === quality &&
      (!target.song || view.qualityPreferences.quality === quality)
    )
      return;
    if (qualityUnavailable(choice)) return;
    if (choice?.access === 'trial_eligible' || choice?.access === 'trial_active') {
      if (details.value?.trialsDamaged) {
        prompt.value = {
          kind: 'notice',
          quality,
          target,
          message: '试用记录无法读取，暂不能启用此音质。你仍可选择其他音质。',
        };
        return;
      }
      prompt.value = {
        kind: choice.access === 'trial_eligible' ? 'confirm' : 'trial',
        quality,
        target,
        choice,
        detailsRevision: details.value!.revision,
      };
      dialogError.value = '';
      if (choice.access === 'trial_active') void useActive();
      return;
    }
    if (choice && ['requires_vip', 'requires_svip', 'requires_purchase'].includes(choice.access)) {
      prompt.value = {
        kind: 'notice',
        quality,
        target,
        choice,
        message:
          choice.access === 'requires_purchase'
            ? '这首歌曲需要购买后才能使用此音质。'
            : choice.access === 'requires_svip'
              ? '此音质需要超级会员权益。'
              : '此音质需要黑胶 VIP 权益。',
      };
      return;
    }
    if (
      target.song &&
      (!choice || choice.access === 'unknown') &&
      !['64aac', 'standard', 'higher', 'exhigh'].includes(quality)
    ) {
      prompt.value = {
        kind: 'notice',
        quality,
        target,
        message: '此音质的使用权益暂未确认，请刷新后重新选择。',
      };
      return;
    }
    void player.action({
      kind: 'quality_for',
      level: quality,
      play_id: target.playId,
      expected_revision: target.revision,
    });
  }
  async function perform(kind: 'start' | 'use_active' | 'reconcile') {
    const selected = prompt.value;
    if (
      !selected ||
      dialogBusy.value ||
      !player.ready.value ||
      selected.target.epoch !== player.view.value?.epoch
    )
      return;
    if (kind !== 'reconcile' && (!same(selected.target, current.value) || player.busy.value))
      return;
    const request = ++operation;
    dialogBusy.value = true;
    dialogError.value = '';
    try {
      let revision = selected.detailsRevision;
      // A playback retry reviews fresh rights, never replays the Start command.
      if (kind === 'use_active') {
        const result = await fetch(selected.target);
        revision = result.revision;
        const choice = result.choices.find((c) => c.quality === selected.quality);
        if (choice?.access === 'allowed') {
          if (alive && request === operation) {
            closePrompt();
            void player.action({
              kind: 'quality_for',
              level: selected.quality,
              play_id: selected.target.playId,
              expected_revision: selected.target.revision,
            });
          }
          return;
        }
        if (choice?.access !== 'trial_active')
          throw {
            code: 'quality_changed',
            message: '音质权益已变化，请重新打开菜单选择',
            retryable: false,
          };
      }
      if (kind !== 'reconcile' && !revision) return;
      const outcome = await player.qualityTrial(
        selected.target.epoch,
        kind === 'reconcile'
          ? { kind, right: selected.trial!.right, operation: selected.trial!.operation }
          : {
              kind,
              play_id: selected.target.playId,
              details_revision: revision!,
              level: selected.quality,
            },
      );
      if (!alive || request !== operation || selected.target.epoch !== player.view.value?.epoch)
        return;
      if (outcome.playback) {
        closePrompt();
        return;
      }
      prompt.value = { ...selected, kind: 'trial', trial: outcome.trial };
      dialogError.value =
        outcome.playbackError?.code === 'stale'
          ? '试用已启用，歌曲或播放状态已变化，请重新选择音质。'
          : (outcome.playbackError?.message ?? '');
    } catch (e) {
      if (alive && request === operation) {
        // Even a storage error after a positive server response can throw.
        // Recover the native journal and never offer an automatic Start retry.
        prompt.value = {
          ...selected,
          kind: 'trial',
          message: '本次操作未完成，请核对试用结果后再选择音质。',
        };
        dialogError.value = failure(e).message;
        try {
          const result = await fetch(selected.target);
          if (alive && request === operation)
            prompt.value = {
              ...prompt.value!,
              trial: result.trials.find((t) => t.right === selected.quality),
            };
        } catch {
          /* Keep the original error; a failed recovery read grants nothing. */
        }
      }
    } finally {
      if (alive && request === operation) dialogBusy.value = false;
    }
  }
  const start = () => (prompt.value?.kind === 'confirm' ? perform('start') : Promise.resolve());
  const useActive = () => perform('use_active');
  const reconcile = () => (prompt.value?.trial ? perform('reconcile') : Promise.resolve());
  watch(
    [() => player.view.value?.epoch, () => current.value?.playId, () => current.value?.revision],
    ([epoch], [oldEpoch]) => {
      hide();
      details.value = undefined;
      captured = undefined;
      if (epoch !== oldEpoch) {
        closePrompt();
        return;
      }
      if (prompt.value?.kind === 'confirm' && !dialogBusy.value) closePrompt();
    },
    { flush: 'sync' },
  );
  function dispose() {
    alive = false;
    hide();
    closePrompt();
  }
  onScopeDispose(dispose);
  return {
    opened,
    details,
    receivedAt,
    loading,
    readError,
    partial,
    prompt,
    dialogBusy,
    dialogError,
    targetChanged,
    show,
    hide,
    refresh,
    choose,
    start,
    useActive,
    reconcile,
    closePrompt,
    dispose,
  };
}
export type PlaybackQualityModel = ReturnType<typeof usePlaybackQuality>;
