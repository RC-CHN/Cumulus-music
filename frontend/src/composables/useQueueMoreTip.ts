import { onScopeDispose, ref, watch, type Ref } from 'vue';
import { useCore } from '../services/core.ts';
import type { PlayerView } from '../services/player.ts';

const SAMPLE_MS = 500;
const SHOW_MS = 5000;

export function useQueueMoreTip(
  view: Readonly<Ref<PlayerView | undefined>>,
  owner: () => string,
  enabled: () => boolean,
) {
  const core = useCore();
  const visible = ref(false);
  let sampleTimer: ReturnType<typeof setTimeout> | undefined;
  let displayTimer: ReturnType<typeof setTimeout> | undefined;
  let attempt: string | undefined;
  let generation = 0;
  let inFlight = false;

  function target(value: PlayerView | undefined) {
    return enabled() &&
      value?.queueMoreTipEligible &&
      value.epoch === owner() &&
      value.playId &&
      value.position != null &&
      Number.isFinite(value.position) &&
      value.position > 0 &&
      value.position <= 30
      ? { epoch: value.epoch, playId: value.playId }
      : undefined;
  }
  function clear() {
    generation++;
    clearTimeout(sampleTimer);
    clearTimeout(displayTimer);
    sampleTimer = displayTimer = undefined;
    attempt = undefined;
    inFlight = false;
    visible.value = false;
  }
  function focused() {
    return document.visibilityState === 'visible' && document.hasFocus();
  }
  async function sample() {
    sampleTimer = undefined;
    const current = target(view.value);
    if (!current || !focused() || inFlight || visible.value || attempt === current.playId) return;
    const ownerGeneration = generation;
    const active = () => ownerGeneration === generation && current.epoch === owner();
    attempt = current.playId;
    inFlight = true;
    try {
      const ticket = await core.queueMoreTip(current.epoch, {
        kind: 'begin',
        play_id: current.playId,
      });
      if (!active()) return;
      if (!ticket) {
        // Native foreground can change between the WebView sample and IPC.
        if (!focused()) attempt = undefined;
        return;
      }
      visible.value = true;
      displayTimer = setTimeout(() => {
        displayTimer = undefined;
        if (!active()) return;
        visible.value = false;
        // This is a local receipt. Do not turn a failed receipt into a player
        // error or retry it from a later account/window.
        void core.queueMoreTip(current.epoch, { kind: 'seen', ticket }).catch(() => {});
      }, SHOW_MS);
    } catch {
      // A rejected or expired playback cue does not affect transport.
    } finally {
      if (active()) inFlight = false;
    }
  }
  watch(owner, clear, { flush: 'sync' });
  watch(
    view,
    (next, previous) => {
      const current = target(next);
      if (
        current &&
        !inFlight &&
        !visible.value &&
        attempt !== current.playId &&
        sampleTimer === undefined &&
        (next?.playId !== previous?.playId || next?.position !== previous?.position)
      ) {
        sampleTimer = setTimeout(() => void sample(), SAMPLE_MS);
      }
    },
    { immediate: true },
  );
  onScopeDispose(clear);
  return visible;
}
