import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { effectScope, shallowRef } from 'vue';
import {
  nicknameFormatIssue,
  nicknameLength,
  usableNicknameRecommendation,
} from '../src/services/nickname.ts';
import {
  useNicknameDraft,
  type NicknameDraftContext,
} from '../src/composables/useNicknameDraft.ts';

const official = JSON.parse(
  readFileSync(
    new URL('../../core/tests/fixtures/reuse-login-official.json', import.meta.url),
    'utf8',
  ),
) as { lengths: Array<{ input: string; length: number }> };

test('nickname lengths match the committed official M32 vectors, including surrogate pairs', () => {
  assert.ok(official.lengths.length >= 5);
  for (const vector of official.lengths)
    assert.equal(nicknameLength(vector.input), vector.length, JSON.stringify(vector.input));
});

test('local nickname format follows official weighted limits and the native character range', () => {
  const cases = [
    ['', 'too_short'],
    ['abc', 'too_short'],
    ['中a', 'too_short'],
    ['abcd', null],
    ['中文', null],
    ['中a文b', null],
    ['ab-c_', null],
    ['a'.repeat(30), null],
    ['a'.repeat(31), 'too_long'],
    ['中'.repeat(15), null],
    ['中'.repeat(16), 'too_long'],
    ['😀😀', 'invalid_characters'],
    ['abé_', 'invalid_characters'],
    ['abĀ', 'invalid_characters'],
    ['abc ', 'too_short'],
    ['ab c', 'invalid_characters'],
    ['abc\u200b', 'invalid_characters'],
    ['昵称\u9fa5', null],
    ['昵称\u9fa6', 'invalid_characters'],
    ['\ufeff\u00a0中文\n', null],
  ] as const;
  for (const [input, issue] of cases)
    assert.equal(nicknameFormatIssue(input), issue, JSON.stringify(input));
  // Length validity does not assert remote nickname availability.
  assert.equal(nicknameFormatIssue('重复昵称'), null);
});

function fixture(suggestion: string | null = '合成推荐昵称') {
  const context = shallowRef<NicknameDraftContext | null>({ owner: 'one:nickname', suggestion });
  let scrubs = 0;
  const scope = effectScope();
  const draft = scope.run(() =>
    useNicknameDraft(
      () => context.value,
      () => scrubs++,
    ),
  )!;
  return { context, draft, scope, scrubs: () => scrubs };
}

test('a valid recommendation replaces a draft even when it equals the prior recommendation', () => {
  const f = fixture();
  f.draft.edit('手工昵称草稿');
  assert.equal(f.draft.acceptRecommendation(f.draft.recommendation(), '合成推荐昵称'), true);
  assert.equal(f.draft.name.value, '合成推荐昵称');
  assert.equal(f.draft.acceptRecommendation(f.draft.recommendation(), '合成推荐昵称'), true);
  assert.equal(f.draft.recommendationError.value, '');
  f.scope.stop();
});

test('missing or invalid recommendations preserve editable text and give bounded feedback', () => {
  const f = fixture();
  f.draft.edit('手工昵称草稿');
  for (const value of [null, undefined, '', 'a', '😀😀']) {
    assert.equal(f.draft.acceptRecommendation(f.draft.recommendation(), value), false);
    assert.equal(f.draft.name.value, '手工昵称草稿');
    assert.equal(f.draft.recommendationError.value, '暂时无法获取推荐昵称，可手动填写');
  }
  f.draft.edit('继续编辑昵称');
  assert.equal(f.draft.name.value, '继续编辑昵称');
  assert.equal(f.draft.recommendationError.value, '');
  f.scope.stop();
});

test('an initial recommendation failure permits manual input without inventing a valid name', () => {
  const f = fixture(null);
  assert.equal(f.draft.name.value, '');
  assert.equal(usableNicknameRecommendation(null), false);
  f.draft.edit('手工填写昵称');
  assert.equal(f.draft.name.value, '手工填写昵称');
  assert.equal(nicknameFormatIssue(f.draft.name.value), null);
  f.scope.stop();
});

test('known duplicate and check failures keep the current nickname-stage draft for correction', () => {
  const f = fixture();
  f.draft.edit('等待检查的昵称');
  f.draft.submitted();
  f.context.value = { owner: 'one:nickname', suggestion: null };
  assert.equal(f.draft.name.value, '等待检查的昵称');
  assert.equal(f.draft.edited.value, false);
  f.draft.edit('修改后再检查');
  assert.equal(f.draft.edited.value, true);
  assert.equal(f.draft.name.value, '修改后再检查');
  f.scope.stop();
});

test('a newer edit or candidate rejects a late recommendation instead of replacing its draft', () => {
  const f = fixture();
  const oldEdit = f.draft.recommendation();
  f.draft.edit('编辑期间的新昵称');
  assert.equal(f.draft.acceptRecommendation(oldEdit, '旧请求推荐昵称'), false);
  assert.equal(f.draft.name.value, '编辑期间的新昵称');
  const oldOwner = f.draft.recommendation();
  const before = f.scrubs();
  f.context.value = { owner: 'two:nickname', suggestion: '新候选推荐昵称' };
  assert.ok(f.scrubs() > before);
  assert.equal(f.draft.name.value, '新候选推荐昵称');
  assert.equal(f.draft.acceptRecommendation(oldOwner, '旧请求推荐昵称'), false);
  assert.equal(f.draft.name.value, '新候选推荐昵称');
  f.scope.stop();
});

test('leaving the nickname stage, resetting and disposing clear drafts and revoke pending hints', () => {
  const f = fixture();
  f.draft.edit('未决注册前的昵称');
  const unknown = f.draft.recommendation();
  const before = f.scrubs();
  f.context.value = null;
  assert.equal(f.draft.name.value, '');
  assert.ok(f.scrubs() > before);
  assert.equal(f.draft.acceptRecommendation(unknown, '迟到昵称'), false);
  f.context.value = { owner: 'three:nickname', suggestion: '新候选昵称' };
  const cancelled = f.draft.recommendation();
  f.draft.clear();
  assert.equal(f.draft.name.value, '');
  assert.equal(f.draft.acceptRecommendation(cancelled, '迟到昵称'), false);
  const closed = f.draft.recommendation();
  f.scope.stop();
  assert.equal(f.draft.name.value, '');
  assert.equal(f.draft.acceptRecommendation(closed, '迟到昵称'), false);
});
