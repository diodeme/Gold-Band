import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import test from 'node:test';

import { repoRoot, tauriConfigOverlay } from './channel-config.mjs';

const baseChannelConfig = {
  productName: 'Gold Band',
  identifier: 'local.gold-band.desktop',
  windowTitle: 'Gold Band',
  updaterPublicKey: 'test-public-key',
  updaterEndpoint: 'https://example.invalid/latest.json',
  allowHttpUpdater: false,
};

test('tauri channel overlay preserves desktop shell window behavior', () => {
  const overlay = tauriConfigOverlay(baseChannelConfig);
  const windowConfig = overlay.app.windows[0];
  const baseTauriConfig = JSON.parse(
    readFileSync(join(repoRoot, 'src-tauri', 'tauri.conf.json'), 'utf8'),
  );

  assert.deepEqual(windowConfig, {
    ...baseTauriConfig.app.windows[0],
    title: baseChannelConfig.windowTitle,
  });
  assert.deepEqual(overlay.app.security, baseTauriConfig.app.security);
});
