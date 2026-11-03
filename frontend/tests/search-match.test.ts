import assert from 'node:assert/strict';
import test from 'node:test';
import { splitSearchMatch } from '../src/services/searchMatch.ts';

test('search text highlights literal repeated matches without altering the title', () => {
  const title = 'İ 🎵 A+B (Live) / a+b (LIVE) <plain>';
  const parts = splitSearchMatch(title, ' a+b   (live) ');
  assert.equal(parts.map((part) => part.text).join(''), title);
  assert.deepEqual(
    parts.filter((part) => part.match).map((part) => part.text),
    ['A+B (Live)', 'a+b (LIVE)'],
  );
  assert.deepEqual(splitSearchMatch('aab (Live)', 'a+b (Live)'), [
    { text: 'aab (Live)', match: false },
  ]);
});

test('search marks are bounded and all remaining text is kept', () => {
  const title = '夜'.repeat(103);
  const parts = splitSearchMatch(title, '夜');
  assert.equal(parts.filter((part) => part.match).length, 100);
  assert.equal(parts.at(-1)?.text, '夜夜夜');
  assert.deepEqual(splitSearchMatch(title, '  '), [{ text: title, match: false }]);
});
