import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { resolve } from 'node:path';

assert(process.env.RECORDING_ATTACHMENTS, 'RECORDING_ATTACHMENTS is required for process output');
const output = resolve(process.env.RECORDING_OUTPUT || 'marketing/site/media');
for (const scene of (process.env.SITE_SCENES || 'before,during,after,personalize').split(',')) {
  assert(['before', 'during', 'after', 'personalize'].includes(scene), `Unknown scene: ${scene}`);
  execFileSync(process.execPath, [resolve('scripts/record-workflow.mjs')], {
    windowsHide: true, stdio: 'inherit',
    env: { ...process.env, RECORDING_SCENE: scene, RECORDING_ATTACHMENTS: resolve(process.env.RECORDING_ATTACHMENTS, scene), RECORDING_OUTPUT: resolve(output, scene === 'during' ? 'workflow' : scene) },
  });
}
