import type { DownloadTask } from './downloads.ts';
import type { MenuItem } from './menu.ts';
import { songLink } from './trackMenu.ts';

export type DownloadFileAction = 'rename' | 'move' | 'delete' | 'forget';
export interface DownloadMenuState {
  locked: boolean;
  playable: boolean;
  queueable: boolean;
}
export function downloadMenuItems(task: DownloadTask, state: DownloadMenuState): MenuItem[] {
  const blocked = state.locked || !!task.fileOperation || task.status === 'cancelling';
  if (task.status !== 'complete')
    return [
      {
        key: 'control',
        label: ['paused', 'error'].includes(task.status) ? '继续下载' : '暂停下载',
        icon: ['paused', 'error'].includes(task.status) ? 'download' : 'pause',
        disabled: blocked,
      },
      { key: 'cancel', label: '删除下载', icon: 'trash', danger: true, disabled: blocked },
    ];
  const voice = task.source === 'voice';
  const link = !voice && songLink(task.track);
  const filesBlocked = blocked || task.cleanupPending;
  return [
    { key: 'play', label: '播放', icon: 'play', disabled: blocked || !state.playable },
    { key: 'next', label: '下一首播放', icon: 'next', disabled: blocked || !state.queueable },
    ...(!voice || task.track.voiceId
      ? [{ key: 'comments', label: '查看评论', icon: 'comment', disabled: state.locked }]
      : []),
    { key: 'resource-divider', label: '', separator: true },
    ...(!voice
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
