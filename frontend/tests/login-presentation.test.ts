import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  phoneError,
  emailSuggestions,
  loadLoginPresentation,
  saveLoginCountry,
  type LoginCountry,
  type LoginPresentationClient,
} from '../src/services/loginPresentation.ts';
const rows: LoginCountry[] = [
  { alpha: 'CN', name: '中国', englishName: 'China', calling: '86', allowedLengths: [11] },
  {
    alpha: 'MY',
    name: '马来西亚',
    englishName: 'Malaysia',
    calling: '60',
    allowedLengths: [9, 10],
  },
  { alpha: 'AC', name: '阿森松', englishName: 'Ascension', calling: '247', allowedLengths: [] },
];
test('official country lengths distinguish strict, multiple and loose number rules', () => {
  assert.equal(phoneError(rows, '13800000000', '86'), '');
  assert.match(phoneError(rows, '1380000000', '86'), /11位/);
  assert.match(phoneError(rows, '1234', '60'), /9或10位/);
  assert.equal(phoneError(rows, '1'.repeat(9), '60'), '');
  assert.equal(phoneError(rows, '1'.repeat(10), '60'), '');
  assert.equal(phoneError(rows, '1111', '247'), '');
  assert.notEqual(phoneError(rows, '１'.repeat(11), '86'), '');
  assert.notEqual(phoneError(rows, '1'.repeat(33), '247'), '');
  assert.notEqual(phoneError(rows, '123', '9999'), '');
});
test('official email suffix order is retained with literal matching and bounded values', () => {
  assert.deepEqual(emailSuggestions('name'), [
    'name@163.com',
    'name@126.com',
    'name@yeah.net',
    'name@vip.163.com',
    'name@vip.126.com',
    'name@188.com',
  ]);
  assert.deepEqual(emailSuggestions('name@vip.'), ['name@vip.163.com', 'name@vip.126.com']);
  assert.deepEqual(emailSuggestions('name@['), []);
  assert.deepEqual(emailSuggestions('name@@163.com'), []);
  assert.deepEqual(emailSuggestions('name @163.com'), []);
  assert.deepEqual(emailSuggestions(''), []);
  assert.deepEqual(emailSuggestions('a'.repeat(254)), []);
});

test('country writes from a closed and reopened modal keep action order and reads wait for persistence', async () => {
  let release!: () => void;
  const held = new Promise<void>((resolve) => {
    release = resolve;
  });
  let selectedCountry = 'CN';
  const calls: string[] = [];
  const core: LoginPresentationClient = {
    loginPresentation: async () => {
      calls.push('read');
      return { countries: rows, selectedCountry };
    },
    selectLoginCountry: async (alpha) => {
      calls.push(alpha);
      if (alpha === 'HK') await held;
      selectedCountry = alpha;
    },
    openLoginDocument: async () => {},
  };
  const oldModal = saveLoginCountry(core, 'HK');
  const newModal = saveLoginCountry(core, 'CA');
  const restored = loadLoginPresentation(core);
  await new Promise(setImmediate);
  assert.deepEqual(calls, ['HK']);
  release();
  await Promise.all([oldModal, newModal]);
  assert.equal((await restored).selectedCountry, 'CA');
  assert.deepEqual(calls, ['HK', 'CA', 'read']);
});

test('a failed region write does not block a later selection or a fresh modal read', async () => {
  let selectedCountry = 'CN';
  const core: LoginPresentationClient = {
    loginPresentation: async () => ({ countries: rows, selectedCountry }),
    selectLoginCountry: async (alpha) => {
      if (alpha === 'HK') throw Error('native save failed');
      selectedCountry = alpha;
    },
    openLoginDocument: async () => {},
  };
  await assert.rejects(saveLoginCountry(core, 'HK'));
  assert.equal((await loadLoginPresentation(core)).selectedCountry, 'CN');
  await saveLoginCountry(core, 'CA');
  assert.equal((await loadLoginPresentation(core)).selectedCountry, 'CA');
});
