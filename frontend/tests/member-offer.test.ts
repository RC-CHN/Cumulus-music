import { test } from 'node:test';
import assert from 'node:assert/strict';
import { effectScope, nextTick, ref } from 'vue';
import { useMemberOffer } from '../src/composables/useMemberOffer.ts';
import type { AccountData } from '../src/services/contracts.ts';
import type { MemberCardOffer, MemberPortalClient } from '../src/services/member.ts';

test('member offers ignore late accounts and refresh stale public handoffs only on request', async () => {
  const owner = ref<string | undefined>('old');
  const resolve = new Map<string, (result: AccountData<MemberCardOffer>) => void>();
  const attempts: [string, string][] = [];
  const reads: [string, boolean][] = [];
  const offer = (epoch: string, revision: string): AccountData<MemberCardOffer> => ({
    epoch,
    value: { mode: 'browser', label: '公开优惠', revision },
  });
  const core: MemberPortalClient = {
    memberPortalQr: async () => assert.fail('browser action cannot load a QR'),
    memberCardOfferQr: async () => assert.fail('browser action cannot load an offer QR'),
    memberCardOffer(epoch, refresh) {
      reads.push([epoch, refresh]);
      return new Promise((done) => resolve.set(epoch, done));
    },
    async openMemberCardOffer(epoch, revision) {
      attempts.push([epoch, revision]);
      throw { code: 'stale', message: 'stale' };
    },
  };
  const scope = effectScope();
  try {
    const model = scope.run(() => useMemberOffer(core, () => owner.value))!;
    owner.value = 'new';
    await nextTick();
    resolve.get('old')!(offer('old', 'old-1'));
    await nextTick();
    assert.equal(model.offer.value, undefined);
    resolve.get('new')!(offer('new', 'new-1'));
    await nextTick();
    await model.openBrowser();
    assert.deepEqual(attempts, [['new', 'new-1']]);
    assert.match(model.issue.value, /重新读取/);
    assert.equal(model.offer.value, undefined);
    const refresh = model.load(true);
    resolve.get('new')!(offer('new', 'new-2'));
    await refresh;
    owner.value = undefined;
    assert.equal(model.offer.value, undefined);
    await model.openBrowser();
    assert.deepEqual(attempts, [['new', 'new-1']]);
    assert.deepEqual(reads, [
      ['old', false],
      ['new', false],
      ['new', true],
    ]);
  } finally {
    scope.stop();
  }
});
