import assert from 'node:assert/strict';
import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import test from 'node:test';

import { readChannelConfig, repoRoot, tauriConfigOverlay } from './channel-config.mjs';

const baseChannelConfig = {
  productName: 'Gold Band',
  mainBinaryName: 'gold-band-desktop',
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

test('support diagnostic overlay disables updater artifacts without changing channel bundle targets', () => {
  const overlay = tauriConfigOverlay(
    { ...baseChannelConfig, bundleTargets: ['nsis'] },
    undefined,
    { createUpdaterArtifacts: false },
  );

  assert.deepEqual(overlay.bundle, {
    publisher: baseChannelConfig.productName,
    targets: ['nsis'],
    createUpdaterArtifacts: false,
  });
});

test('channel overlay sets Windows publisher from product name', () => {
  const overlay = tauriConfigOverlay(baseChannelConfig);

  assert.equal(overlay.productName, 'Gold Band');
  assert.equal(overlay.bundle.publisher, 'Gold Band');
});

test('wb overlay embeds MALING as both product name and publisher', () => {
  const overlay = tauriConfigOverlay(readChannelConfig('wb'));

  assert.equal(overlay.productName, 'MALING');
  assert.equal(overlay.bundle.publisher, 'MALING');
});

test('channel overlay sets the configured main binary name', () => {
  assert.equal(tauriConfigOverlay(baseChannelConfig).mainBinaryName, 'gold-band-desktop');
  assert.equal(tauriConfigOverlay(readChannelConfig('wb')).mainBinaryName, 'maling-desktop');
});

test('every channel uses a distinct main binary name so installers only close their own channel', () => {
  const channels = readdirSync(join(repoRoot, 'configs', 'channels'))
    .filter((file) => file.endsWith('.json'))
    .map((file) => file.slice(0, -'.json'.length));
  const names = channels.map((channel) => readChannelConfig(channel).mainBinaryName.toLowerCase());

  assert.ok(channels.length > 1);
  assert.equal(new Set(names).size, names.length);
});
