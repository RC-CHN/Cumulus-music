import { test } from 'node:test';
import assert from 'node:assert/strict';
import { nicknameError, reuseDate, reuseTitle } from '../src/services/phoneReuse.ts';
test('phone reuse keeps official login and registration titles for each pending step', () => {
  assert.equal(reuseTitle('choice'), '确认账号');
  assert.equal(reuseTitle('confirming'), '登录当前账号');
  assert.equal(reuseTitle('login_failed'), '登录当前账号');
  assert.equal(reuseTitle('register_consent'), '注册新账号');
  assert.equal(reuseTitle('registering'), '希望大家怎么称呼你呢？');
  assert.equal(reuseTitle('nickname'), '希望大家怎么称呼你呢？');
  assert.equal(reuseTitle('recovery_required'), '确认账号');
});
test('official creation dates preserve calendar-only fields and use local calendar for timestamps', () => {
  assert.equal(reuseDate('2021-03-04'), '2021-03-04');
  const d = new Date(2021, 2, 4, 14, 30);
  assert.equal(reuseDate(d.getTime()), '2021-03-04');
  assert.equal(reuseDate(d.toISOString()), '2021-03-04');
  for (const v of [null, undefined, '', 'invalid date', NaN, Infinity])
    assert.equal(reuseDate(v), '');
});
test('nickname feedback distinguishes a duplicate from an unconfirmed readonly check', () => {
  assert.equal(nicknameError('taken'), '昵称已使用');
  assert.equal(nicknameError('check_failed'), '暂时无法检查昵称，请重试');
  assert.equal(nicknameError(null), '');
  assert.match(nicknameError('invalid_characters'), /-_/);
});
