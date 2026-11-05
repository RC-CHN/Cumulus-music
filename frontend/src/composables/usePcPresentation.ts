import type { AccountView } from '../services/contracts.ts';
import type { PcPresentationClient, PcPresentationView } from '../services/pcPresentation.ts';
import type { SearchViewportMemory } from '../services/searchViewport.ts';

/** One shell subscription outlives search pages and their Back snapshots. */
export function usePcPresentation(core: PcPresentationClient, memory: SearchViewportMemory) {
  let account: AccountView | undefined;
  let generation = 0,
    eventSerial = 0;
  let closed = false,
    started = false,
    listening = false;
  let unlisten: (() => void) | undefined;
  function observe(view: PcPresentationView) {
    if (closed || account?.status !== 'authenticated' || account.epoch !== view.epoch) return;
    eventSerial++;
    memory.observePresentation(view);
  }
  async function snapshot() {
    if (!listening || account?.status !== 'authenticated') return;
    const epoch = account.epoch,
      visit = generation,
      serial = eventSerial;
    try {
      const view = await core.pcPresentation(epoch);
      if (
        !closed &&
        generation === visit &&
        account?.epoch === epoch &&
        eventSerial === serial &&
        view.epoch === epoch
      )
        memory.observePresentation(view);
    } catch {
      // A local observation failure cannot cause a search, login or polling loop.
    }
  }
  function connect(next: AccountView) {
    if (next.epoch === account?.epoch && next.status === account.status) return;
    generation++;
    account = next;
    void snapshot();
  }
  async function start() {
    if (closed || started) return;
    started = true;
    try {
      const stop = await core.onPcPresentationChanged(observe);
      if (closed) {
        stop();
        return;
      }
      unlisten = stop;
      listening = true;
      // Subscribe first: events during this local snapshot invalidate its reply.
      await snapshot();
    } catch {
      // Existing native search reads still gate their own presentation.
    }
  }
  function dispose() {
    closed = true;
    generation++;
    listening = false;
    unlisten?.();
    unlisten = undefined;
  }
  return { connect, start, dispose };
}
