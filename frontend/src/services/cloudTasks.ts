import type { CloudUpload } from './cloud.ts';
export interface CloudTaskTarget {
  epoch: string;
  ids: string[];
  titles: string[];
  all: boolean;
  history: boolean;
  retry?: boolean;
}
export function taskProgress(task: CloudUpload) {
  const attachments = task.attachments.filter((a) => a.phase !== 'omitted');
  const total = task.total + attachments.reduce((n, a) => n + a.total, 0),
    bytes = Math.min(
      total,
      task.bytes + attachments.reduce((n, a) => n + Math.min(a.bytes, a.total), 0),
    ),
    processing = [
      'metadata_pending',
      'encoding',
      'ready_to_publish',
      'publication_pending',
      'published',
    ].includes(task.phase),
    active = task.enabled || task.running;
  // Recovery state and publication come from the native task journal. The
  // byte count and worker's active flag cannot distinguish these states.
  const nativeStatus = ['等待核对', '无法核对上传结果', '取消待确认', '正在保存'].includes(
    task.label,
  );
  return {
    total,
    bytes,
    percent: total ? (bytes / total) * 100 : 0,
    processing,
    showProgress: !nativeStatus && !task.removalPending && !processing && !task.error,
    label: nativeStatus
      ? task.label
      : task.removalPending
        ? '正在移除'
        : task.error
          ? task.error === 'encoding_failed'
            ? '转码失败'
            : '上传未完成'
          : task.phase === 'complete'
            ? '已上传'
            : task.phase === 'cancelled'
              ? '已取消'
              : task.label === '已暂停' || !active
                ? '已暂停'
                : processing
                  ? '正在转码'
                  : ['queued', 'checked', 'allocated'].includes(task.phase)
                    ? '排队中'
                    : `${(total ? (bytes / total) * 100 : 0).toFixed(1)}%`,
  };
}
export function cloudTaskTarget(
  epoch: string,
  rows: CloudUpload[],
  all = false,
  history = false,
): CloudTaskTarget {
  return {
    epoch,
    ids: rows.map((r) => r.id),
    titles: rows.map((r) =>
      r.title && r.title !== r.fileName ? `${r.title} · ${r.fileName}` : r.fileName,
    ),
    all,
    history,
    retry: rows.some((row) => row.removalPending),
  };
}
export function uploadRowMenu(disabled: boolean) {
  return [
    { key: 'play', label: '播放', icon: 'play', disabled: true },
    { key: 'next', label: '下一首播放', icon: 'next', disabled: true },
    { key: 'separator', label: '', separator: true },
    { key: 'directory', label: '打开文件所在目录', icon: 'folder', disabled },
  ];
}
