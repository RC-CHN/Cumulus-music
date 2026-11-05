import { computed, ref } from 'vue';
import { failure } from '../services/contracts.ts';
import { sampleSceneCover, type SceneCoverPixels } from '../services/sceneFmCanvas.ts';
import {
  sceneFmBackgroundCss,
  type SceneFmBackground,
  type SceneFmPaletteClient,
  type SceneFmPaletteRead,
} from '../services/sceneFmPalette.ts';

export interface SceneFmPaletteScope {
  epoch: string;
  revision: string | null | undefined;
  voiceId: string | null | undefined;
  // This is the public artwork identity already in SceneFmView. It is never
  // passed to native fetch or loaded by this composable.
  coverKey: string | null | undefined;
}
type Sample = Extract<SceneFmPaletteRead, { kind: 'sample' }>;
type Decoder = (image: Sample, signal: AbortSignal) => Promise<SceneCoverPixels>;

export function useSceneFmPalette(core: SceneFmPaletteClient, decode: Decoder = sampleSceneCover) {
  const background = ref<SceneFmBackground>({ kind: 'theme_fallback' });
  const pending = ref(false);
  let current: SceneFmPaletteScope | undefined;
  let generation = 0;
  let requested = false;
  let disposed = false;
  let abort: AbortController | undefined;
  let receipt: { epoch: string; id: string } | undefined;

  function cancelReceipt() {
    const previous = receipt;
    receipt = undefined;
    if (previous)
      void core.sceneFmPaletteCancel(previous.epoch, previous.id).catch(() => {
        // Obsolete native receipts cannot change the newer card.
      });
  }
  async function show(next: SceneFmPaletteScope, pixelRatio: number) {
    if (disposed) return;
    const previous = current;
    current = { ...next };
    // The official hook depends on image URL, not playback pause or view
    // revision. Native admission validates the starting voice; the in-flight
    // artwork and receipt remain valid for the same current account and URL.
    if (previous?.epoch === next.epoch && previous.coverKey === next.coverKey && requested) return;
    const serial = ++generation;
    requested = false;
    abort?.abort();
    cancelReceipt();
    if (previous?.epoch !== next.epoch) background.value = { kind: 'theme_fallback' };
    pending.value = false;
    if (!next.revision || !next.voiceId || !next.coverKey) return;
    requested = true;
    const controller = new AbortController();
    abort = controller;
    const live = () => !disposed && serial === generation && !controller.signal.aborted;
    pending.value = true;
    try {
      const read = await core.sceneFmPaletteRead(
        next.epoch,
        next.revision,
        next.voiceId,
        pixelRatio,
      );
      if (!live() || read.epoch !== next.epoch) {
        if (live()) requested = false;
        if (read.value.kind === 'sample')
          void core.sceneFmPaletteCancel(next.epoch, read.value.receipt).catch(() => {});
        return;
      }
      if (read.value.kind === 'ready') {
        background.value = read.value.background;
        return;
      }
      const source = read.value;
      receipt = { epoch: next.epoch, id: source.receipt };
      const pixels = await decode(source, controller.signal);
      if (!live()) return;
      const sampled = await core.sceneFmPaletteSample(
        next.epoch,
        source.receipt,
        pixels.width,
        pixels.height,
        pixels.rgba,
      );
      if (!live() || sampled.epoch !== next.epoch) {
        if (live()) requested = false;
        return;
      }
      receipt = undefined; // A successful native completion consumes the receipt.
      background.value = sampled.value.background;
    } catch (cause) {
      if (live()) {
        // M54 catches ordinary metadata/image failures and resolves its default
        // color. Native account/source cancellation is not a completed color:
        // a fresh authoritative scope must still be able to request it.
        if (['stale', 'profile_changed', 'session_expired'].includes(failure(cause).code))
          requested = false;
        background.value = { kind: 'theme_fallback' };
      }
    } finally {
      if (live()) {
        cancelReceipt();
        pending.value = false;
        abort = undefined;
      }
    }
  }
  function dispose() {
    disposed = true;
    generation++;
    abort?.abort();
    abort = undefined;
    cancelReceipt();
    pending.value = false;
    background.value = { kind: 'theme_fallback' };
  }
  return {
    background,
    css: computed(() => sceneFmBackgroundCss(background.value)),
    pending,
    show,
    dispose,
  };
}
