import { computed, ref, shallowRef, watch } from 'vue';
import { failure } from '../services/contracts.ts';
import { equalizerChange, equalizerToneEnabled } from '../services/equalizer.ts';
import type {
  EqualizerPreset,
  EqualizerSettings,
  EqualizerView,
  PlayerAction,
} from '../services/player.ts';

type EqualizerWrite = Extract<PlayerAction, { kind: 'equalizer' }>;
type Current = { epoch: string; equalizer: EqualizerView };

function samePreset(a?: EqualizerPreset | null, b?: EqualizerPreset | null) {
  return (
    a?.kind === b?.kind &&
    (a?.kind !== 'built_in' || (b?.kind === 'built_in' && a.index === b.index))
  );
}

function same(a: EqualizerSettings, b: EqualizerSettings) {
  return (
    !!a.master_bypassed === !!b.master_bypassed &&
    a.enabled === b.enabled &&
    equalizerToneEnabled(a) === equalizerToneEnabled(b) &&
    (a.bass_db ?? 0) === (b.bass_db ?? 0) &&
    (a.treble_db ?? 0) === (b.treble_db ?? 0) &&
    (a.presence ?? 0) === (b.presence ?? 0) &&
    (a.stereoizer ?? 0) === (b.stereoizer ?? 0) &&
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
  const draft = shallowRef<{ settings: EqualizerSettings; preset?: EqualizerPreset }>();
  const pending = ref(false);
  const discarding = ref(false);
  const blocked = ref(false);
  const error = ref('');
  const settings = computed(() => draft.value?.settings ?? current()?.equalizer.settings);
  const preset = computed(() => (draft.value ? draft.value.preset : current()?.equalizer.preset));
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
    discarding.value = false;
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

  function stage(next: EqualizerSettings, selected = preset.value ?? undefined) {
    const value = current();
    if (
      !value ||
      discarding.value ||
      value.equalizer.damaged ||
      (settings.value && same(settings.value, next) && samePreset(preset.value, selected))
    )
      return;
    if (!draft.value) revision = value.equalizer.revision;
    draft.value = { settings: { ...next, gains: [...next.gains] }, preset: selected };
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
      if (recover && matches(fresh.equalizer, draft.value) && !fresh.equalizer.damaged) {
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
      const next = await write(equalizerChange(value, submitted.settings, submitted.preset));
      if (!active()) return;
      if (
        next.epoch !== base.epoch ||
        next.equalizer.damaged ||
        !matches(next.equalizer, submitted)
      )
        throw new Error('无法确认均衡器设置已保存，请读取当前设置后重试。');
      revision = next.equalizer.revision;
      if (draft.value && matches(next.equalizer, draft.value)) draft.value = undefined;
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

  function matches(view: EqualizerView, selected: NonNullable<typeof draft.value>) {
    return same(view.settings, selected.settings) && samePreset(view.preset, selected.preset);
  }

  async function discard() {
    const base = current();
    if (!base || !blocked.value || !draft.value || pending.value) return;
    const owner = generation;
    const active = () => generation === owner && current()?.epoch === base.epoch;
    pending.value = discarding.value = true;
    try {
      // Discard only the renderer's draft. A lost write response can hide a
      // successful native commit, so cached settings cannot replace it.
      const fresh = await read();
      if (!active()) return;
      if (fresh.epoch !== base.epoch) throw new Error('无法确认当前账号的均衡器设置。');
      reset();
    } catch (issue) {
      if (active()) {
        error.value = failure(issue).message;
        pending.value = discarding.value = false;
      }
    }
  }

  return {
    settings,
    preset,
    draft,
    pending,
    discarding,
    blocked,
    error,
    stage,
    applyDraft: () => send(true),
    discard,
    reset,
  };
}
