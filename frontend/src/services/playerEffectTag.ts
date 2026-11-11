import type { AccountView, TrackView } from './contracts.ts';
import type { EffectMinibarEntry } from './pcPresentation.ts';
import type { PlayerView } from './player.ts';
import { equalizerTone, equalizerToneEnabled } from './equalizer.ts';

type AudioEffectPlayback = Pick<
  PlayerView,
  | 'epoch'
  | 'resourceKind'
  | 'equalizer'
  | 'deviceParametric'
  | 'boundParametric'
  | 'appliedEffectId'
> & { track?: Pick<TrackView, 'voiceId'> | null };

type EffectLabel = '音效' | '均衡器' | '耳机音效';
type Tag = { kind: 'legacy' | 'modern'; label: EffectLabel };

function selectedLabel(playback: AudioEffectPlayback, signedIn: boolean): EffectLabel | undefined {
  const labels: EffectLabel[] = [];
  const effect = playback.equalizer;
  if (!effect.damaged && !effect.settings.master_bypassed) {
    const tone = equalizerTone(effect.settings);
    const selectedGeqOrTone =
      (effect.status === 'configured' || effect.status === 'processing') &&
      (effect.settings.enabled ||
        (equalizerToneEnabled(effect.settings) && (tone.bass_db !== 0 || tone.treble_db !== 0)));
    const selectedSpatial =
      effect.spatial_status === 'configured' || effect.spatial_status === 'processing';
    if (selectedGeqOrTone || selectedSpatial) labels.push('均衡器');
  }
  if (signedIn) {
    const headphone = playback.deviceParametric?.status;
    if (headphone === 'configured' || headphone === 'bypassed_for_source') labels.push('耳机音效');
    if (
      playback.appliedEffectId != null &&
      (playback.boundParametric === 'configured' ||
        playback.boundParametric === 'bypassed_for_source')
    )
      labels.push('音效');
  }
  // The stock client has one currentAudioEffect. Independent native lanes
  // here can coexist; no recovered priority or unseen catalogue title is inferred.
  return labels.length === 1 ? labels[0] : labels.length ? '音效' : undefined;
}

export function playerEffectTag(
  cohort: EffectMinibarEntry | undefined,
  account: Pick<AccountView, 'epoch' | 'status'>,
  view: AudioEffectPlayback | undefined,
): Tag | undefined {
  if (cohort === undefined || cohort === 'legacy') return { kind: 'legacy', label: '音效' };
  if (cohort === 'hidden') return;
  const playback = view?.epoch === account.epoch ? view : undefined;
  if (
    playback?.resourceKind === 'voice' ||
    (playback?.resourceKind === 'downloaded' && playback.track?.voiceId)
  )
    return;
  const active = playback && selectedLabel(playback, account.status === 'authenticated');
  if (cohort === 'active_only' && !active) return;
  return { kind: 'modern', label: active ?? '音效' };
}
