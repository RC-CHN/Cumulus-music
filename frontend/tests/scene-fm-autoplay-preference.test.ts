import { test } from 'node:test';
import assert from 'node:assert/strict';
import { useSceneFmAutoplay } from '../src/composables/useSceneFmAutoplay.ts';
import type { AccountData } from '../src/services/contracts.ts';

const account = (epoch: string) => ({
  epoch,
  status: 'authenticated' as const,
  nickname: 'test',
  avatarUrl: null,
});

test('播客自动播放只保存本机偏好，丢失回执后只回读一次', async () => {
  const calls: string[] = [];
  let saved = false;
  const model = useSceneFmAutoplay({
    async sceneFmAutoplayPreference(epoch) {
      calls.push(`read:${epoch}`);
      return { epoch, value: saved };
    },
    async setSceneFmAutoplayPreference(epoch, enabled) {
      calls.push(`write:${epoch}:${enabled}`);
      saved = enabled;
      throw { code: 'unavailable', message: '未确认保存' };
    },
  });
  await model.connect(account('1'));
  assert.equal(model.enabled.value, false);
  assert.equal(await model.change(true), false);
  assert.equal(model.enabled.value, true);
  assert.equal(model.saved.value, false);
  assert.equal(model.error.value, '未确认保存');
  assert.equal(model.canChange.value, false);
  assert.deepEqual(calls, ['read:1', 'write:1:true', 'read:1']);
  await model.read();
  assert.equal(model.canChange.value, true);
  assert.equal(calls.at(-1), 'read:1');
  model.dispose();
});

test('换号后旧保存回执不能改新账号的开关或状态', async () => {
  let finish!: (value: AccountData<boolean>) => void;
  const model = useSceneFmAutoplay({
    async sceneFmAutoplayPreference(epoch) {
      return { epoch, value: false };
    },
    setSceneFmAutoplayPreference() {
      return new Promise((resolve) => (finish = resolve));
    },
  });
  await model.connect(account('1'));
  const old = model.change(true);
  await model.connect(account('2'));
  finish({ epoch: '1', value: true });
  assert.equal(await old, false);
  assert.equal(model.enabled.value, false);
  assert.equal(model.saved.value, false);
  assert.equal(model.canChange.value, true);
  model.dispose();
});
