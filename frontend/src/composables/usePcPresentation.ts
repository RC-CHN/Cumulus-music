import { readonly, ref } from 'vue';
import type { AccountView } from '../services/contracts.ts';
import type {
  EffectCataloguePresentation,
  EffectMinibarEntry,
  PcPresentationClient,
  PcPresentationView,
} from '../services/pcPresentation.ts';
import { presentationRevision, type SearchViewportMemory } from '../services/searchViewport.ts';

/** One shell subscription outlives search pages and their Back snapshots. */
export function usePcPresentation(core: PcPresentationClient, memory: SearchViewportMemory) {
  let account: AccountView | undefined;
  let generation = 0;
  let closed = false,
    started = false,
    listening = false;
  let unlisten: (() => void) | undefined;
  let guestOrder: bigint | undefined;
  const guestSearchReady = ref(false);
  const podcastResume = ref(false);
  const podcastPurchasesCompact = ref(false);
  const effectCatalogue = ref<EffectCataloguePresentation>();
  const effectMinibarEntry = ref<EffectMinibarEntry>('legacy');
  let signedOrder: bigint | undefined;
  const eligibleGuest = () => account?.status === 'unknown' || account?.status === 'signed_out';
  function observe(view: PcPresentationView) {
    if (closed || account?.epoch !== view.epoch) return;
    if (account.status === 'authenticated') {
      memory.observePresentation(view);
      const order = presentationRevision(view.revision);
      if (order !== undefined && (signedOrder === undefined || order > signedOrder)) {
        signedOrder = order;
        podcastResume.value = view.podcastResume === true;
        podcastPurchasesCompact.value = view.podcastPurchasesCompact === true;
        // yxrk can change while the catalogue layout and its revision stay
        // fixed. Consume only the general presentation revision for the tag.
        effectMinibarEntry.value =
          view.effectMinibarEntry ?? (view.effectCatalogueNew ? 'hidden' : 'legacy');
        const cohort = presentationRevision(view.effectCatalogueRevision);
        const previous = presentationRevision(effectCatalogue.value?.revision);
        if (
          view.effectCatalogueRevision &&
          cohort !== undefined &&
          (previous === undefined || cohort > previous)
        ) {
          effectCatalogue.value = {
            revision: view.effectCatalogueRevision,
            layout: view.effectCatalogueNew ? 'modern' : 'legacy',
          };
        }
      }
    } else if (eligibleGuest()) {
      const order = presentationRevision(view.revision);
      if (order === undefined || (guestOrder !== undefined && order <= guestOrder)) return;
      guestOrder = order;
      guestSearchReady.value = view.guestSearchReady === true;
    }
  }
  async function snapshot() {
    if (!listening || !account || (account.status !== 'authenticated' && !eligibleGuest())) return;
    const epoch = account.epoch,
      visit = generation;
    try {
      const view = await core.pcPresentation(epoch);
      if (!closed && generation === visit && account?.epoch === epoch && view.epoch === epoch)
        observe(view);
    } catch {
      // A local observation failure cannot cause a search, login or polling loop.
    }
  }
  function connect(next: AccountView) {
    if (next.epoch === account?.epoch && next.status === account.status) return;
    generation++;
    account = next;
    guestOrder = undefined;
    signedOrder = undefined;
    podcastResume.value = false;
    podcastPurchasesCompact.value = false;
    effectCatalogue.value = undefined;
    effectMinibarEntry.value = 'legacy';
    guestSearchReady.value = false;
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
      // Subscribe first. Memory arbitrates both sources by native revision,
      // including a newer snapshot that completes after an intermediate event.
      await snapshot();
    } catch {
      // Existing native search reads still gate their own presentation.
    }
  }
  function dispose() {
    closed = true;
    generation++;
    guestSearchReady.value = false;
    listening = false;
    podcastResume.value = false;
    podcastPurchasesCompact.value = false;
    effectCatalogue.value = undefined;
    effectMinibarEntry.value = 'legacy';
    unlisten?.();
    unlisten = undefined;
  }
  return {
    connect,
    start,
    dispose,
    guestSearchReady: readonly(guestSearchReady),
    podcastResume: readonly(podcastResume),
    podcastPurchasesCompact: readonly(podcastPurchasesCompact),
    effectCatalogue: readonly(effectCatalogue),
    effectMinibarEntry: readonly(effectMinibarEntry),
  };
}
