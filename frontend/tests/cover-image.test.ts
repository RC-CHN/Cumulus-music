import assert from 'node:assert/strict';
import { test } from 'node:test';
import { effectScope, shallowRef } from 'vue';
import { useCoverImage } from '../src/composables/useCoverImage.ts';

function fixture() {
  const scope = effectScope();
  const current = shallowRef({
    owner: 'account-1',
    playId: 'play-a',
    voiceId: '601',
    source: 'https://p1.music.126.net/a.png',
  });
  const image = scope.run(() => useCoverImage(() => current.value))!;
  const event = (generation = image.generation.value, url = image.source.value) =>
    ({
      currentTarget: {
        getAttribute(name: string) {
          return name === 'src'
            ? url
            : name === 'data-cover-generation'
              ? String(generation)
              : null;
        },
      },
    }) as unknown as Event;
  return { scope, current, image, event };
}

test('a cover loads once per source and selects a local placeholder after error', () => {
  const f = fixture();
  assert.equal(f.image.state.value, 'loading');
  assert.equal(f.image.loaded.value, false);
  assert.equal(f.image.showImage.value, true);
  f.image.error(f.event());
  assert.equal(f.image.state.value, 'error');
  assert.equal(f.image.showImage.value, false);
  const generation = f.image.generation.value;
  f.current.value = { ...f.current.value };
  assert.equal(f.image.generation.value, generation);
  assert.equal(f.image.state.value, 'error');
  f.scope.stop();
});

test('a different source resets the placeholder and ignores late load/error from the previous image', () => {
  const f = fixture(),
    old = f.event();
  f.image.error(old);
  f.current.value = { ...f.current.value, source: 'https://p1.music.126.net/b.png' };
  assert.equal(f.image.state.value, 'loading');
  f.image.load(old);
  f.image.error(old);
  assert.equal(f.image.state.value, 'loading');
  f.image.load(f.event());
  assert.equal(f.image.loaded.value, true);
  assert.equal(f.image.showImage.value, true);
  f.scope.stop();
});

test('an account change invalidates even a late event with the identical cover URL', () => {
  const f = fixture(),
    old = f.event();
  f.image.load(old);
  f.current.value = { ...f.current.value, owner: 'account-2' };
  assert.equal(f.image.state.value, 'loading');
  f.image.error(old);
  assert.equal(f.image.state.value, 'loading');
  f.image.load(f.event());
  assert.equal(f.image.state.value, 'ready');
  f.scope.stop();
});

test('empty sources and disposed components expose no image or late state changes', () => {
  const f = fixture(),
    old = f.event();
  f.current.value = { ...f.current.value, source: '' };
  assert.equal(f.image.state.value, 'empty');
  assert.equal(f.image.source.value, null);
  assert.equal(f.image.showImage.value, false);
  f.image.load(old);
  assert.equal(f.image.state.value, 'empty');
  f.scope.stop();
  f.current.value = { ...f.current.value, source: 'https://p1.music.126.net/c.png' };
  f.image.error(old);
  assert.equal(f.image.state.value, 'empty');
});

test('a new play and voice keep both ready and failed state for the same account and URL', () => {
  for (const outcome of ['ready', 'error'] as const) {
    const f = fixture();
    if (outcome === 'ready') f.image.load(f.event());
    else f.image.error(f.event());
    const generation = f.image.generation.value;
    f.current.value = { ...f.current.value, playId: 'play-b', voiceId: '602' };
    assert.equal(f.image.generation.value, generation);
    assert.equal(f.image.state.value, outcome);
    assert.equal(f.image.loaded.value, outcome === 'ready');
    assert.equal(f.image.showImage.value, outcome === 'ready');
    f.scope.stop();
  }
});

test('an in-flight image for the same public URL may complete after the voice changes', () => {
  const f = fixture(),
    originalNode = f.event();
  const generation = f.image.generation.value;
  f.current.value = { ...f.current.value, playId: 'play-b', voiceId: '602' };
  assert.equal(f.image.generation.value, generation);
  f.image.load(originalNode);
  assert.equal(f.image.state.value, 'ready');
  f.scope.stop();
});
