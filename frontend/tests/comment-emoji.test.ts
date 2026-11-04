import assert from 'node:assert/strict';
import { test } from 'node:test';
import { commentEmoji, commentTextParts } from '../src/services/commentEmoji.ts';

test('comment rendering converts only known bracket labels and preserves neighboring text', () => {
  assert.equal(new Set(commentEmoji.map(({ label }) => label)).size, 60);
  assert.ok(commentEmoji.every(({ preview }) => preview));
  assert.deepEqual(commentTextParts('🎵[大笑][心碎] [表情]\n<img>[未知]'), [
    { kind: 'text', text: '🎵' },
    { kind: 'emoji', label: '大笑', preview: '😄' },
    { kind: 'emoji', label: '心碎', preview: '💔' },
    { kind: 'text', text: ' [表情]\n<img>[未知]' },
  ]);
});
