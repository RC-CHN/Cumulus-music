import test from 'node:test';
import assert from 'node:assert/strict';
import { downloadAdmissionSummary } from '../src/services/downloadFeedback.ts';
import type { DownloadResult } from '../src/services/downloads.ts';

test('existing ordinary task is not described as a new acceptance', () => {
  const result: DownloadResult = {
    epoch: '1',
    operationId: null,
    accepted: [],
    failed: [],
    skipped: [{ id: '123', reason: 'options_changed' }],
  };
  assert.equal(
    downloadAdmissionSummary(result),
    '现有任务的设置不同；可按原设置继续，或取消后按新设置下载',
  );
  result.skipped = [{ id: '123', reason: 'downloaded' }];
  assert.equal(downloadAdmissionSummary(result), '资源已下载');
});

test('mixed admission separates accepted IDs, existing resources and errors', () => {
  const result: DownloadResult = {
    epoch: '1',
    operationId: null,
    accepted: ['new-native-task'],
    failed: [{ id: '127', message: '请核对目录' }],
    skipped: [
      { id: '123', reason: 'waiting' },
      { id: '124', reason: 'local' },
      { id: '125', reason: 'waiting' },
    ],
  };
  assert.equal(
    downloadAdmissionSummary(result),
    '已受理 1 项；2 项：等待下载中；1 项：已有对应本地文件；1 项未完成：请核对目录',
  );
  result.skipped = [];
  assert.equal(downloadAdmissionSummary(result), undefined);
});
