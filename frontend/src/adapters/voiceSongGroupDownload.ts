import type {
  VoiceSongGroupDestinationCommand,
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
      invoke('player_voice_songs_download_prepare_with_destination', {
        epoch,
        playId,
        cardReceipt,
        anchorIndex,
        indices: [...indices],
      }),
    playerVoiceSongsDownloadDestination: (target, command) =>
      invoke('player_voice_songs_download_destination', {
        target: prepared(target),
        command: destinationCommand(command),
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

function destinationCommand(command: VoiceSongGroupDestinationCommand) {
  const { kind } = command;
  switch (kind) {
    case 'page':
      return { kind, sort: command.sort, page: command.page };
    case 'choose':
      return { kind, playlistId: command.playlistId };
    case 'choose_liked':
      return { kind, destinationReceipt: command.destinationReceipt };
    case 'status':
    case 'creation_status':
      return { kind, reconcile: command.reconcile };
    case 'create':
      return { kind, name: command.name, privatePlaylist: command.privatePlaylist };
    case 'start':
      return { kind, continuation: command.continuation };
    case 'liked_destination':
    case 'add_created':
      return { kind };
  }
}
