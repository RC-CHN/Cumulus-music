export interface NativeLikeChanged {
  epoch: string;
  resource: { kind: 'song' | 'voice'; id: string };
}
export interface NativeLikesClient {
  onNativeLikeChanged(handler: (value: NativeLikeChanged) => void): Promise<() => void>;
}
