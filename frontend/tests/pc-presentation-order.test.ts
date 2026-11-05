import assert from 'node:assert/strict';
import test from 'node:test';
import { createSearchViewportMemory } from '../src/services/searchViewport.ts';
import type { PcPresentationView } from '../src/services/pcPresentation.ts';
import type { AccountView } from '../src/services/contracts.ts';

const account: AccountView = {
  epoch: '1',
  status: 'authenticated',
  nickname: null,
  avatarUrl: null,
};
const appearance = (revision: string, epoch = '1'): PcPresentationView => ({
  epoch,
  revision,
  searchFooter: true,
  searchExpanded: true,
  searchDense: false,
});

test('same-owner revision ordering is lossless across the full native u64 range', () => {
  const memory = createSearchViewportMemory();
  memory.connect(account);
  let changed = 0;
  memory.onPresentation(() => {
    changed++;
  });
  assert.equal(memory.observePresentation(appearance('0')), 'changed');
  assert.equal(memory.observePresentation(appearance('0')), 'unchanged');
  assert.equal(memory.observePresentation({ ...appearance('0'), searchDense: true }), 'rejected');
  assert.equal(changed, 1);
  for (const invalid of [
    '',
    '-1',
    '+1',
    '00',
    '01',
    ' 1',
    '1 ',
    '1.0',
    '1e2',
    '0x10',
    '18446744073709551616',
    '99999999999999999999999999999',
  ])
    assert.equal(memory.observePresentation(appearance(invalid)), 'rejected', invalid);
  assert.equal(memory.presentation('1')?.revision, '0');
  assert.equal(changed, 1);
  assert.equal(
    memory.observePresentation({ ...appearance('1'), revision: 1 as unknown as string }),
    'rejected',
  );
  assert.equal(memory.observePresentation(appearance('9007199254740992')), 'changed');
  assert.equal(memory.observePresentation(appearance('9007199254740993')), 'changed');
  assert.equal(memory.observePresentation(appearance('9007199254740992')), 'rejected');
  memory.clear();
  assert.equal(
    memory.observePresentation(appearance('1')),
    'rejected',
    'clearing browsing rows cannot reset the same-owner native clock',
  );
  const maximum = appearance('18446744073709551615');
  assert.equal(memory.observePresentation(maximum), 'changed');
  maximum.searchFooter = false;
  assert.equal(
    memory.presentation('1')?.searchFooter,
    true,
    'later mutation of an input cannot alter accepted state',
  );
  assert.equal(memory.observePresentation(appearance('18446744073709551614')), 'rejected');
  assert.equal(memory.presentation('1')?.revision, '18446744073709551615');
  assert.equal(changed, 4);
});

test('a new account resets revision ordering and rejects late views from the old owner', () => {
  const memory = createSearchViewportMemory();
  memory.connect(account);
  memory.observePresentation(appearance('18446744073709551615'));
  memory.connect({ ...account, epoch: '2' });
  assert.equal(memory.observePresentation(appearance('0', '2')), 'changed');
  assert.equal(memory.observePresentation(appearance('18446744073709551615')), 'rejected');
  assert.equal(memory.presentation('2')?.revision, '0');
  assert.equal(memory.presentation('1'), undefined);
  memory.connect({ ...account, epoch: '2', status: 'signed_out' });
  assert.equal(memory.observePresentation(appearance('1', '2')), 'rejected');
});
