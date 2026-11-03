import { computed, ref, shallowRef, watch } from 'vue';
import { failure } from '../services/contracts.ts';
import { equalizerChange } from '../services/equalizer.ts';
import type { EqualizerSettings, EqualizerView, PlayerAction } from '../services/player.ts';

type EqualizerWrite = Extract<PlayerAction, { kind: 'equalizer' }>;
type Current = { epoch: string; equalizer: EqualizerView };

function same(a: EqualizerSettings, b: EqualizerSettings) {
  return (
    a.enabled === b.enabled &&
    (a.bass_db ?? 0) === (b.bass_db ?? 0) &&
    (a.treble_db ?? 0) === (b.treble_db ?? 0) &&
    a.gains.length === b.gains.length &&
    a.gains.every((gain, i) => Math.fround(gain) === Math.fround(b.gains[i]!))
  );
}

/** One acknowledged CAS at a time. Keep the newest local curve until native
 * confirms it; never replace its base revision silently after a failed write. */
export function useEqualizerDraft(
  current: () => Current | undefined,
  write: (command: EqualizerWrite) => Promise<Current>,
  read: () => Promise<Current>,
  delay = 90,
) {
  const draft = shallowRef<EqualizerSettings>();
  const pending = ref(false);
  const blocked = ref(false);
  const error = ref('');
  const settings = computed(() => draft.value ?? current()?.equalizer.settings);
  let timer: ReturnType<typeof setTimeout> | undefined;
  let revision: string | undefined;
  let generation = 0;

  function reset() {
    generation++;
    clearTimeout(timer);
    timer = undefined;
    draft.value = undefined;
    revision = undefined;
    pending.value = false;
    blocked.value = false;
    error.value = '';
  }

  watch(() => current()?.epoch, reset, { flush: 'sync' });

  function schedule() {
    if (timer !== undefined || pending.value || blocked.value || !draft.value) return;
    timer = setTimeout(() => {
      timer = undefined;
      void send(false);
    }, delay);
  }

  function stage(next: EqualizerSettings) {
    const value = current();
    if (!value || value.equalizer.damaged || (settings.value && same(settings.value, next))) return;
    if (!draft.value) revision = value.equalizer.revision;
    draft.value = { ...next, gains: [...next.gains] };
    schedule();
  }

  async function send(recover: boolean) {
    const base = current();
    if (!base || !draft.value || pending.value || blocked.value !== recover) return;
    const owner = generation;
    const active = () => generation === owner && current()?.epoch === base.epoch;
    pending.value = true;
    try {
      const fresh = recover ? await read() : base;
      if (!active() || !draft.value) return;
      if (fresh.epoch !== base.epoch) throw new Error('无法确认当前账号的均衡器设置。');
      if (recover && same(fresh.equalizer.settings, draft.value) && !fresh.equalizer.damaged) {
        // A reply may have been lost after a successful durable write.
        draft.value = undefined;
        blocked.value = false;
        error.value = '';
        return;
      }
      const value = {
        ...fresh.equalizer,
        revision: recover ? fresh.equalizer.revision : revision!,
      };
      const submitted = draft.value;
      const next = await write(equalizerChange(value, submitted));
      if (!active()) return;
      if (
        next.epoch !== base.epoch ||
        next.equalizer.damaged ||
        !same(submitted, next.equalizer.settings)
      )
        throw new Error('无法确认均衡器设置已保存，请读取当前设置后重试。');
      revision = next.equalizer.revision;
      if (draft.value && same(draft.value, next.equalizer.settings)) draft.value = undefined;
      blocked.value = false;
      error.value = '';
    } catch (issue) {
      if (!active()) return;
      blocked.value = true;
      error.value = failure(issue).message;
    } finally {
      if (active()) {
        pending.value = false;
        schedule();
      }
    }
  }

  return {
    settings,
    draft,
    pending,
    blocked,
    error,
    stage,
    applyDraft: () => send(true),
    discard: () => {
      if (!pending.value) reset();
    },
    reset,
  };
}
