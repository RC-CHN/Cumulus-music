import assert from 'node:assert/strict';
import test from 'node:test';
import { downloadProgressStatus, type DownloadTask } from '../src/services/downloads.ts';

test('active download speed uses the decimal desktop display thresholds', () => {
  const text = (speedBytesPerSecond: number) =>
    downloadProgressStatus({ status: 'downloading', speedBytesPerSecond } as DownloadTask, true);
  for (const [rate, expected] of [
    [0, '0 B/s'],
    [999, '999 B/s'],
    [1_000, '1 KB/s'],
    [12_666, '12 KB/s'],
    [999_999, '1000 KB/s'],
    [1_000_000, '1.0 MB/s'],
    [1_234_567, '1.2 MB/s'],
  ] as const)
    assert.equal(text(rate), expected);
});

test('paused, stale and unmeasured transfers keep their own state text', () => {
  const active = { status: 'downloading', speedBytesPerSecond: 13_000 } as DownloadTask;
  assert.equal(downloadProgressStatus(active, false), '正在下载');
  assert.equal(downloadProgressStatus({ ...active, speedBytesPerSecond: null }, true), '正在下载');
  assert.equal(downloadProgressStatus({ ...active, status: 'paused' }, true), '已暂停');
  assert.equal(downloadProgressStatus({ ...active, status: 'error' }, true), '下载失败，可重试');
});
