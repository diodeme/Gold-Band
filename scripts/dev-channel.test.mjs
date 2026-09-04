import assert from 'node:assert/strict';
import { join } from 'node:path';
import test from 'node:test';

import { repoRoot } from './channel-config.mjs';
import { devChannelEnvironment } from './dev-channel.mjs';
import { readFileSync } from 'node:fs';

test('default dev serializes Cargo builds without overriding explicit limits', () => {
  assert.deepEqual(
    devChannelEnvironment({ EXISTING: 'kept' }, 'default'),
    {
      EXISTING: 'kept',
      GOLD_BAND_RELEASE_CHANNEL: 'default',
      CARGO_BUILD_JOBS: '1',
    },
  );

  assert.equal(
    devChannelEnvironment({ CARGO_BUILD_JOBS: '4' }, 'default').CARGO_BUILD_JOBS,
    '4',
  );
});

test('package exposes the default-channel dev command', () => {
  const packageJson = JSON.parse(readFileSync(join(repoRoot, 'package.json'), 'utf8'));

  assert.equal(
    packageJson.scripts.dev,
    'node scripts/dev-channel.mjs default',
  );
});
