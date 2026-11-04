import assert from 'node:assert/strict';
import { test } from 'node:test';
import { topicParts } from '../src/services/communityTopic.ts';

test('complete literal note hashtags become internal labels while unsafe text stays text', () => {
  const input = '<b>#听歌 100%#</b> 与 #第二个话题#，非法 #坏/路径#';
  const parts = topicParts(input);
  assert.equal(parts.map((part) => part.text).join(''), input);
  assert.deepEqual(
    parts.flatMap((part) => (part.topic ? [part.topic] : [])),
    ['听歌 100%', '第二个话题'],
  );
  assert.equal(
    topicParts(Array.from({ length: 29 }, (_, index) => `#标签${index}#`).join(' ')).filter(
      (part) => part.topic,
    ).length,
    24,
  );
});
