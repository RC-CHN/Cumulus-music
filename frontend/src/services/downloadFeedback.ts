import type { DownloadCommand, DownloadControl } from './downloads';

const control: Record<DownloadControl, string> = {
  pause: '暂停下载',
  resume: '继续下载',
  cancel: '取消下载',
  repair: '修复下载文件',
};
/** Retain the operation's meaning even if its result arrives on another tab. */
export function downloadCommandLabel(command: DownloadCommand): string {
  switch (command.kind) {
    case 'start':
      return `下载歌曲（${command.song_ids.length}首）`;
    case 'start_voice':
      return `下载声音（${command.voice_ids.length}条）`;
    case 'start_mv':
      return '下载MV';
    case 'start_cloud':
      return `下载云盘文件（${command.song_ids.length}项）`;
    case 'control':
      return `${control[command.action]}（${command.task_ids.length}项）`;
    case 'all':
      return command.action === 'pause'
        ? '暂停全部下载'
        : command.action === 'resume'
          ? '继续全部下载'
          : '清空下载任务';
    case 'preferences':
      return '保存下载设置';
    case 'configure':
      return '保存下载目录';
    case 'migrate':
      return '迁移下载目录';
    case 'migration':
      return command.cancel ? '取消目录迁移' : '继续目录迁移';
    case 'file':
      return command.action === 'delete'
        ? '删除下载文件'
        : command.action === 'forget'
          ? '移除下载记录'
          : '移动下载文件';
    case 'rename':
      return '重命名下载文件';
    case 'move_to_directory':
      return '移动下载文件';
    case 'file_operation':
      return command.cancel ? '取消文件处理' : '继续文件处理';
    case 'stop_operation':
      return '停止文件处理';
  }
}
