import type { AccountData, TrackView } from './contracts';
import type { PlayerView, Quality } from './player';

export interface ArtistOffer {
  id: string;
  scene: 'new_user' | 'recall_user';
  resumed: boolean;
  saveBlocked: boolean;
}
export interface ArtistCandidate {
  id: string;
  name: string;
  coverUrl: string | null;
}
export type ArtistSave = 'confirmed' | 'uncertain';
export type ArtistResultAction = 'play_now' | 'later';

export interface ArtistOnboardingClient {
  artistOffer(epoch: string): Promise<AccountData<ArtistOffer | null>>;
  artistCandidates(
    epoch: string,
    id: string,
    similarTo?: string,
  ): Promise<AccountData<ArtistCandidate[]>>;
  artistSave(epoch: string, id: string, artists: string[]): Promise<AccountData<ArtistSave>>;
  artistPreview(epoch: string, id: string): Promise<AccountData<TrackView[]>>;
  artistClose(epoch: string, id: string): Promise<void>;
  artistResult(
    epoch: string,
    id: string,
    action: ArtistResultAction,
    queueRevision: string,
    quality: Quality,
  ): Promise<PlayerView>;
}
