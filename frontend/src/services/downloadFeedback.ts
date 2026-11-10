import type {
  DownloadCommand,
  DownloadControl,
  DownloadResult,
  DownloadSkipReason,
} from './downloads';

const admission: Record<DownloadSkipReason, string> = {
  downloaded: '资源已下载',
  local: '已有对应本地文件',
  waiting: '等待下载中',
  working: '现有任务正在处理',
  cancelling: '请等待取消完成后再下载',
  options_changed: '现有任务的设置不同；可按原设置继续，或取消后按新设置下载',
  existing: '已有任务，请到下载管理继续或修复',
  missing: '原下载文件缺失或已变化，请核对下载记录',
  conflict: '已有多条下载记录，请先核对',
};

export function downloadAdmissionSummary(result: DownloadResult): string | undefined {
  const skipped = result.skipped;
  if (!skipped?.length) return;
  const counts = new Map<DownloadSkipReason, number>();
  for (const { reason } of skipped) counts.set(reason, (counts.get(reason) ?? 0) + 1);
  return [
    ...(result.accepted.length ? [`已受理 ${result.accepted.length} 项`] : []),
    ...[...counts].map(([reason, count]) =>
      skipped.length === 1 ? admission[reason] : `${count} 项：${admission[reason]}`,
    ),
    ...(result.failed.length
      ? [`${result.failed.length} 项未完成：${result.failed[0]?.message}`]
      : []),
  ].join('；');
}

const control: Record<DownloadControl, string> = {
  pause: '暂停下载',
  resume: '继续下载',
  cancel: '取消下载',
  repair: '修复下载文件',
};
/** Retain the operation's meaning even if its result arrives on another tab. */
export function downloadCommandLabel(command: DownloadCommand): string {
  switch (command.kind) {
    case 'restart_failed_transfer':
      return '从头下载';
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
      return command.action === 'pause' ? '暂停全部下载' : '继续全部下载';
    case 'confirm_clear':
      return '清空下载任务';
    case 'preferences':
      return '保存下载设置';
    case 'configure':
      return '保存下载目录';
    case 'migrate':
      return '迁移下载目录';
    case 'migration':
      return command.cancel ? '取消目录迁移' : '继续目录迁移';
    case 'delete_files':
      return `删除下载文件（${command.task_ids.length}项）`;
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
