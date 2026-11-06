/** Native presentation observations; these grant no playback or account rights. */
export interface PcPresentationView {
  epoch: string;
  revision: string;
  searchFooter: boolean;
  searchExpanded: boolean;
  searchDense: boolean;
  guestSearchReady?: boolean;
}
export interface PcPresentationClient {
  pcPresentation(epoch: string): Promise<PcPresentationView>;
  onPcPresentationChanged(listener: (view: PcPresentationView) => void): Promise<() => void>;
}
