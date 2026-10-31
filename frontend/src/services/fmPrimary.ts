export type FmPrimaryState = {
  active: boolean;
  playing: boolean;
  modePending: boolean;
  transportActive: boolean;
  hasCurrent: boolean;
  pending: boolean;
  notice: boolean;
};

// Label, icon and command describe one intent, including the interval where the
// domain queue changed but the audible queue still awaits native recovery.
export function fmPrimary(state: FmPrimaryState) {
  if (!state.active) return { action: 'start', label: '开始漫游', icon: 'play' } as const;
  if (state.modePending)
    return { action: 'resume', label: '同步并继续播放', icon: 'repeat' } as const;
  if (state.playing) return { action: 'pause', label: '暂停漫游', icon: 'pause' } as const;
  if (!state.hasCurrent)
    return { action: 'resume', label: '重新获取推荐', icon: 'repeat' } as const;
  if (!state.transportActive || state.pending || state.notice)
    return { action: 'resume', label: '继续播放', icon: 'play' } as const;
  return { action: 'play', label: '继续播放', icon: 'play' } as const;
}
