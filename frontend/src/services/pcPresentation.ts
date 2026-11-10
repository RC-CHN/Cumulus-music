/** Native presentation observations; these grant no playback or account rights. */
export interface PcPresentationView {
  epoch: string;
  revision: string;
  searchFooter: boolean;
  searchExpanded: boolean;
  searchDense: boolean;
  guestSearchReady?: boolean;
  podcastResume?: boolean;
  effectCatalogueRevision?: string;
  effectCatalogueNew?: boolean;
}
export interface PcPresentationClient {
  pcPresentation(epoch: string): Promise<PcPresentationView>;
  onPcPresentationChanged(listener: (view: PcPresentationView) => void): Promise<() => void>;
}

export interface EffectCataloguePresentation {
  revision: string;
  layout: 'legacy' | 'modern';
}
