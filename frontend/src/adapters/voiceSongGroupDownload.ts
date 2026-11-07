import type {
  VoiceSongGroupDownloadClient,
  VoiceSongGroupDownloadTarget,
} from '../services/voiceSongs.ts';

type Invoke = <T>(command: string, payload: Record<string, unknown>) => Promise<T>;

export function createVoiceSongGroupDownloadBridge(invoke: Invoke): VoiceSongGroupDownloadClient {
  const prepared = ({
    epoch,
    playId,
    cardReceipt,
    selectionReceipt,
  }: VoiceSongGroupDownloadTarget) => ({ epoch, playId, cardReceipt, selectionReceipt });

  return {
    playerVoiceSongsDownloadPrepare: ({ epoch, playId, cardReceipt, anchorIndex, indices }) =>
      invoke('player_voice_songs_download_prepare', {
        epoch,
        playId,
        cardReceipt,
        anchorIndex,
        indices: [...indices],
      }),
    playerVoiceSongsDownloadCancel: (target) =>
      invoke('player_voice_songs_download_cancel', prepared(target)),
    playerVoiceSongsDownloadStart: (target) =>
      invoke('player_voice_songs_download_start', prepared(target)),
    playerVoiceSongsDownloadConfirmQuality: (target, quality) =>
      invoke('player_voice_songs_download_confirm_quality', { ...prepared(target), quality }),
    openVoiceSongGroupPurchasedQualityGuide: (target) =>
      invoke('open_voice_song_group_purchased_quality_guide', prepared(target)),
  };
}
