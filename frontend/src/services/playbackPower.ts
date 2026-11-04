/** Availability combines the native platform and the official experiment. */
export interface PlaybackPowerView {
  available: boolean;
  enabled: boolean;
}
export interface PlaybackPowerClient {
  playbackPower(epoch: string): Promise<PlaybackPowerView>;
  setPlaybackPower(epoch: string, enabled: boolean): Promise<PlaybackPowerView>;
}
