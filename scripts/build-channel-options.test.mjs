import assert from 'node:assert/strict';
import test from 'node:test';

import {
  channelBuildPlan,
  parseChannelBuildArgs,
  SUPPORT_DEVTOOLS_FEATURE,
} from './build-channel-options.mjs';

test('channel build options keep the default release path unchanged', () => {
  assert.deepEqual(parseChannelBuildArgs(['default']), {
    channel: 'default',
    isCritical: false,
    devtools: false,
  });
  assert.deepEqual(channelBuildPlan('tauri.default.conf.json'), {
    tauriArgs: ['tauri', 'build', '--config', 'tauri.default.conf.json'],
    tauriConfigBuildOptions: undefined,
    shouldCollectReleaseArtifacts: true,
  });
});

test('channel build options enable release-profile support devtools independently from critical updates', () => {
  assert.deepEqual(parseChannelBuildArgs(['wb', '--critical', '--devtools']), {
    channel: 'wb',
    isCritical: true,
    devtools: true,
  });
  assert.deepEqual(channelBuildPlan('tauri.wb.conf.json', { devtools: true }), {
    tauriArgs: [
      'tauri',
      'build',
      '--config',
      'tauri.wb.conf.json',
      '--features',
      SUPPORT_DEVTOOLS_FEATURE,
    ],
    tauriConfigBuildOptions: { createUpdaterArtifacts: false },
    shouldCollectReleaseArtifacts: false,
  });
});

test('channel build options retain the legacy positional critical flag and reject typos', () => {
  assert.equal(parseChannelBuildArgs(['wb', 'critical']).isCritical, true);
  assert.throws(
    () => parseChannelBuildArgs(['wb', '--devtool']),
    /Unsupported build option: --devtool/,
  );
});
