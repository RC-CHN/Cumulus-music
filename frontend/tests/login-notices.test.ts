import { test } from 'node:test';
import assert from 'node:assert/strict';
import type { LoginView } from '../src/services/auth.ts';
import { presentLoginNotice } from '../src/services/loginPresentation.ts';

test('risk notice retains the native title and ordered paragraphs as plain text', () => {
  const notice = {
    kind: 'risk' as const,
    title: '合成风险提示',
    paragraphs: ['第一段', '<a href="https://example.invalid">合成文本</a>', '第三段'],
  };
  assert.deepEqual(presentLoginNotice({ status: 'risk_notice', notice }), notice);
});

test('a notice is shown only for its current matching terminal login status', () => {
  const notice = { kind: 'risk' as const, title: '旧提示', paragraphs: ['旧内容'] };
  const statuses: LoginView['status'][] = [
    'ready',
    'captcha',
    'two_factor',
    'security_question',
    'security_qr',
    'phone_reuse',
    'phone_entry',
    'rejected',
    'authenticated',
  ];
  for (const status of statuses) assert.equal(presentLoginNotice({ status, notice }), null);
  assert.deepEqual(presentLoginNotice({ status: 'upgrade_required', notice }), {
    kind: 'upgrade',
    title: '版本更新提醒',
    paragraphs: [],
  });
});

test('304 uses the official prohibition text without asserting its duration', () => {
  assert.deepEqual(
    presentLoginNotice({
      status: 'account_disabled',
      notice: { kind: 'disabled', title: '合成的其他标题', paragraphs: [] },
    }),
    { kind: 'disabled', title: '用户禁止登录', paragraphs: [] },
  );
});

test('older views without an optional notice still present bounded login guidance', () => {
  assert.equal(presentLoginNotice(undefined), null);
  assert.deepEqual(presentLoginNotice({ status: 'risk_notice' }), {
    kind: 'risk',
    title: '登录风险提示',
    paragraphs: [],
  });
  assert.deepEqual(presentLoginNotice({ status: 'upgrade_required', notice: null }), {
    kind: 'upgrade',
    title: '版本更新提醒',
    paragraphs: [],
  });
});
