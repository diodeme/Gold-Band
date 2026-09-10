import assert from 'node:assert/strict';
import test from 'node:test';
import { referenceAnimationsSettled } from './site-reference-readiness.mjs';

const animation = (playState, endTime, top = 0) => ({
  playState,
  effect: {
    target: { getBoundingClientRect: () => ({ width: 300, height: 80, top, bottom: top + 80 }) },
    getComputedTiming: () => ({ endTime }),
  },
});
const ready = (...animations) => referenceAnimationsSettled({ getAnimations: () => animations }, 844);

test('reference capture waits for visible finite entrances even when navigation is ready', () => {
  const navigation = animation('finished', 300);
  const hero = animation('running', 1540);
  assert.equal(ready(navigation, hero), false);
  hero.playState = 'finished';
  assert.equal(ready(navigation, hero), true);
});

test('reference capture does not wait for infinite decoration or offscreen entrances', () => {
  assert.equal(ready(animation('running', Infinity), animation('running', 1540, 900)), true);
  assert.equal(ready(animation('running', 1540, -100)), true);
  assert.equal(ready(animation('paused', 1540)), false);
  assert.equal(ready(animation('idle', 1540)), true);
});
