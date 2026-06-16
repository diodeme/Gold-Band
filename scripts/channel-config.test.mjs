import assert from 'node:assert/strict';
import test from 'node:test';

import { tauriConfigOverlay } from './channel-config.mjs';

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

  assert.equal(windowConfig.decorations, false);
  assert.equal(windowConfig.dragDropEnabled, false);
});
