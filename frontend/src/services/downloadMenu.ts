import { downloadPlaybackReady, type DownloadShareAccess, type DownloadTask } from './downloads.ts';
import type { CommentResource } from './comments.ts';
import type { TrackView } from './contracts.ts';
import type { MenuItem } from './menu.ts';

export type DownloadFileAction = 'rename' | 'move' | 'delete' | 'forget';
export interface DownloadMenuState {
  locked: boolean;
  playable: boolean;
  queueable: boolean;
  commentCount?: number;
}
export function downloadMenuCommentResource(task: DownloadTask): CommentResource | undefined {
  if (task.status !== 'complete') return undefined;
  if (task.source === 'voice' && task.commentVoiceId)
    return { kind: 'voice', id: task.commentVoiceId };
  if ((task.source === 'song' || task.source === 'cloud') && task.commentTrackId)
    return { kind: 'track', id: task.commentTrackId };
}
export function downloadMenuShareTrack(task: DownloadTask): TrackView | undefined {
  if (task.status !== 'complete' || !['song', 'cloud'].includes(task.source)) return;
  const id = task.linkTrackId;
  if (id && /^[1-9][0-9]*$/.test(id)) return { ...task.track, id, local: false };
}
export function downloadShareRestriction(access: DownloadShareAccess): string | undefined {
  if (access === 'allowed') return;
  if (access === 'album_required') return '该歌曲需要购买数字专辑，暂不能分享';
  return '分享失败，该歌曲暂无分享版权';
}
export function downloadMenuItems(task: DownloadTask, state: DownloadMenuState): MenuItem[] {
  const blocked = state.locked || !!task.fileOperation || task.status === 'cancelling';
  if (task.status !== 'complete')
    return [
      {
        key: 'control',
        label: task.needsTransferRestart
          ? '从头下载'
          : ['paused', 'error'].includes(task.status)
            ? '继续下载'
            : '暂停下载',
        icon: ['paused', 'error'].includes(task.status) ? 'download' : 'pause',
        disabled: blocked,
      },
      { key: 'cancel', label: '删除下载', icon: 'trash', danger: true, disabled: blocked },
    ];
  const comment = downloadMenuCommentResource(task);
  const link = downloadMenuShareTrack(task);
  const filesBlocked = blocked || task.cleanupPending;
  const commentCount = state.commentCount ?? 0;
  const comments =
    comment && Number.isSafeInteger(commentCount) && commentCount > 0
      ? '查看评论(' + commentCount + ')'
      : '查看评论';
  return [
    {
      key: 'play',
      label: '播放',
      icon: 'play',
      disabled: blocked || !state.playable || !downloadPlaybackReady(task),
    },
    {
      key: 'next',
      label: '下一首播放',
      icon: 'next',
      disabled: blocked || !state.queueable || !downloadPlaybackReady(task),
    },
    ...(comment
      ? [{ key: 'comments', label: comments, icon: 'comment', disabled: state.locked }]
      : []),
    { key: 'resource-divider', label: '', separator: true },
    ...(task.collectionAvailable && (task.source === 'song' || task.source === 'cloud')
      ? [{ key: 'collect', label: '收藏', icon: 'star', disabled: blocked || task.track.missing }]
      : []),
    ...(link
      ? [
          { key: 'share', label: '分享', icon: 'share', disabled: state.locked },
          { key: 'copy', label: '复制链接', icon: 'copy', disabled: state.locked },
        ]
      : []),
    { key: 'open', label: '打开文件所在目录', icon: 'list', disabled: blocked || !task.path },
    { key: 'delete-divider', label: '', separator: true },
    { key: 'delete', label: '删除下载', icon: 'trash', danger: true, disabled: filesBlocked },
    // Cumulus file-management additions after the recovered resource actions.
    { key: 'file-divider', label: '', separator: true },
    { key: 'rename', label: '重命名…', disabled: filesBlocked },
    { key: 'move', label: '移动到…', disabled: filesBlocked },
    { key: 'forget', label: '仅移出下载列表…', disabled: filesBlocked },
    { key: 'manage', label: '管理下载文件…', icon: 'more', disabled: state.locked },
  ];
}
