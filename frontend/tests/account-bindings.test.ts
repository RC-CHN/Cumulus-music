import assert from 'node:assert/strict';
import { test } from 'node:test';
import { bindingGroups } from '../src/services/accountBindings.ts';

test('unknown registration never promotes a known binding or offers to unlink it', () => {
  const [group, ...others] = bindingGroups({
    registrationProvider: null,
    foreignRegisteredPhone: false,
    bindings: [
      { provider: 'email', typeCode: 0, maskedAddress: 'f***@e***', detailsAvailable: true },
      { provider: 'wechat', typeCode: 10, detailsAvailable: false },
      {
        provider: 'cellphone',
        typeCode: 1,
        maskedAddress: '***1234',
        countryCode: '86',
        hasPassword: true,
        detailsAvailable: true,
      },
    ],
  });
  assert.deepEqual(others, []);
  assert.equal(group.key, 'unclassified');
  assert.deepEqual(
    group.rows.map((row) => [row.method, row.registered, row.change, row.password, row.mayBind]),
    [
      ['cellphone', false, '更换手机号', '修改密码', false],
      ['email', false, undefined, undefined, false],
      ['wechat', false, undefined, undefined, false],
      ['qq', false, undefined, undefined, true],
      ['weibo', false, undefined, undefined, true],
    ],
  );
});
