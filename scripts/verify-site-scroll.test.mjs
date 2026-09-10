import { test } from 'node:test';
import assert from 'node:assert/strict';
import { verifyScroll } from './verify-site-scroll.mjs';

const before = { paused: true, scrollY: 1000, mediaTop: 150, frame: { checkpoint: 'menu', transform: 'scale(1)', body: 'recorded content' } };
const after = { ...before, scrollY: 1040, active: 'during', players: 1, samePlayer: true, overflow: false, paused: true,
  sticky: 'sticky', headerBottom: 88, mediaRight: 700, copyLeft: 740 };
test('accepts sticky desktop and naturally scrolling mobile measurements', () => {
  verifyScroll(before, after, true);
  verifyScroll(before, { ...after, mediaTop: 110, sticky: null, mediaBottom: 350, copyTop: 380 }, false);
});
test('rejects missing scroll, clock advancement, remounts, overlaps and incorrect positioning', () => {
  assert.throws(() => verifyScroll({ ...before, paused: false }, after, true));
  for (const change of [{ scrollY: 1000 }, { frame: { ...before.frame, body: 'next event' } }, { samePlayer: false },
    { mediaTop: 110 }, { mediaTop: 70 }, { mediaRight: 760 }, { active: 'after' }, { players: 2 }]) {
    assert.throws(() => verifyScroll(before, { ...after, ...change }, true));
  }
  assert.throws(() => verifyScroll(before, { ...after, sticky: null, mediaBottom: 350, copyTop: 380 }, false));
  assert.throws(() => verifyScroll(before, { ...after, mediaTop: 110, sticky: null, mediaBottom: 400, copyTop: 380 }, false));
});
