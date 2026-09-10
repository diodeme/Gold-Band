import { test } from 'node:test';
import assert from 'node:assert/strict';
import { summarizeMotion } from './verify-site-motion.mjs';

const sample = (checkpoint, at) => ({ checkpoint, at, players: 1, textLength: 100, overflow: false, camera: at % 2 ? 'moving' : 'stable', transform: `matrix-${at}` });
test('continuous acceptance requires ordered complete steps, an observed loop and intermediate camera frames', () => {
  const samples = ['opening', 'a', 'a', 'a', 'b', 'b', 'closing', 'opening'].map(sample);
  assert.equal(summarizeMotion(samples, ['opening', 'a', 'b', 'closing']).transforms, 8);
  assert.throws(() => summarizeMotion(samples.filter(s=>s.checkpoint!=='b'), ['opening', 'a', 'b', 'closing']), /skipped or reordered/);
  assert.throws(() => summarizeMotion(samples.slice(0, -1), ['opening', 'a', 'b', 'closing']), /Loop restart/);
  assert.throws(() => summarizeMotion(samples.map(s=>({...s, players:2})), ['opening', 'a', 'b', 'closing']), /duplicate/);
  assert.throws(() => summarizeMotion(samples.map(s=>({...s, camera:'stable'})), ['opening', 'a', 'b', 'closing']), /transition/);
});
