import { computed, nextTick, shallowRef, watch } from 'vue';
import type { PlaybackPowerClient } from '../services/playbackPower';
import type { SystemMediaClient } from '../services/systemMedia';

export type HeaderGuideKind = 'system_media' | 'keep_awake';
const seenKeys = {
  system_media: 'cumulus.playback.system-media-guide-seen.v1',
  keep_awake: 'cumulus.playback.keep-awake-guide-seen.v1',
} satisfies Record<HeaderGuideKind, string>;
const stepDelay = 3000; // M/70 checks SMTC first, then waits another 3s for keep-awake.
let shownInProcess = false;

function seen(kind: HeaderGuideKind) {
  try {
    return localStorage.getItem(seenKeys[kind]) === '1';
  } catch {
    return false;
  }
}

function rememberPresentation(kind: HeaderGuideKind) {
  shownInProcess = true;
  try {
    localStorage.setItem(seenKeys[kind], '1');
  } catch {
    // The launch still does not show either guide again.
  }
}

export function useHeaderSettingsGuide(
  core: Pick<PlaybackPowerClient, 'playbackPower'> &
    Pick<SystemMediaClient, 'systemMediaGuideAvailable'>,
  owner: () => string,
  route: () => { kind: string } | undefined,
  blocked: () => boolean,
) {
  const offer = shallowRef<{ epoch: string; kind: HeaderGuideKind; presented: boolean }>();
  const visible = computed(() =>
    offer.value?.epoch === owner() && !blocked() ? offer.value : undefined,
  );

  watch(
    [owner, route],
    ([epoch, page], _, onCleanup) => {
      if (offer.value?.epoch !== epoch || !offer.value.presented) offer.value = undefined;
      if (!page || shownInProcess || (seen('system_media') && seen('keep_awake'))) return;
      let current = true;
      const valid = () =>
        current && owner() === epoch && route() === page && !blocked() && !shownInProcess;
      async function present(kind: HeaderGuideKind) {
        if (!valid() || seen(kind)) return false;
        const candidate = { epoch, kind, presented: false };
        offer.value = candidate;
        await nextTick();
        if (offer.value !== candidate) return false;
        if (!valid()) {
          offer.value = undefined;
          return false;
        }
        rememberPresentation(kind);
        offer.value = { ...candidate, presented: true };
        return true;
      }
      let timer = setTimeout(async () => {
        if (!valid()) return;
        try {
          if (
            !seen('system_media') &&
            (await core.systemMediaGuideAvailable(epoch)) &&
            (await present('system_media'))
          )
            return;
          if (!valid() || page.kind !== 'discovery_home' || seen('keep_awake')) return;
          timer = setTimeout(async () => {
            if (!valid()) return;
            try {
              const power = await core.playbackPower(epoch);
              if (power.available && !power.enabled) await present('keep_awake');
            } catch {
              // A failed read never consumes an introduction.
            }
          }, stepDelay);
        } catch {
          // An ambiguous higher-priority result cannot promote the next guide.
        }
      }, stepDelay);
      onCleanup(() => {
        current = false;
        clearTimeout(timer);
      });
    },
    { immediate: true },
  );

  function dismiss() {
    const dismissed = visible.value;
    offer.value = undefined;
    return dismissed;
  }
  return { visible, dismiss };
}
