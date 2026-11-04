import { computed, nextTick, shallowRef, watch } from 'vue';
import type { PlaybackPowerClient } from '../services/playbackPower.ts';

const seenKey = 'cumulus.playback.keep-awake-guide-seen.v1';
const guideDelay = 6000; // M/70: the preceding SMTC check and this check each wait 3 s.
let shownInProcess = false;

function seen() {
  try {
    return shownInProcess || localStorage.getItem(seenKey) === '1';
  } catch {
    return shownInProcess;
  }
}

function rememberPresentation() {
  shownInProcess = true;
  try {
    localStorage.setItem(seenKey, '1');
  } catch {
    // The launch still does not show the same guide twice.
  }
}

export function usePlaybackPowerGuide(
  core: Pick<PlaybackPowerClient, 'playbackPower'>,
  owner: () => string,
  home: () => boolean,
  blocked: () => boolean,
) {
  const offer = shallowRef<{ epoch: string; presented: boolean }>();
  const visible = computed(
    () => offer.value?.epoch === owner() && !blocked() && (offer.value?.presented || home()),
  );

  watch(
    [owner, home],
    ([epoch, isHome], _, onCleanup) => {
      if (offer.value?.epoch !== epoch || !offer.value.presented) offer.value = undefined;
      if (!isHome || seen()) return;
      let current = true;
      const valid = () => current && owner() === epoch && home() && !blocked() && !seen();
      const timer = setTimeout(async () => {
        if (!valid()) return;
        try {
          const power = await core.playbackPower(epoch);
          if (
            !valid() ||
            !power.available ||
            power.enabled ||
            power.guideDeferredForSystemMedia !== false
          )
            return;
          const candidate = { epoch, presented: false };
          offer.value = candidate;
          await nextTick();
          if (offer.value !== candidate) return;
          if (!valid()) {
            offer.value = undefined;
            return;
          }
          rememberPresentation();
          offer.value = { epoch, presented: true };
        } catch {
          // A failed native read never consumes or fabricates an introduction.
        }
      }, guideDelay);
      onCleanup(() => {
        current = false;
        clearTimeout(timer);
      });
    },
    { immediate: true },
  );

  function dismiss() {
    const epoch = visible.value ? offer.value?.epoch : undefined;
    offer.value = undefined;
    return epoch;
  }
  return { visible, dismiss };
}
