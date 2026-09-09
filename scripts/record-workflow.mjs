import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { openSync, closeSync, readFileSync } from 'node:fs';
import { copyFile, mkdir, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { workflowSourceStoryboard, WORKFLOW_CAMERA_TRANSITION_MS } from './workflow-source-storyboard.mjs';
import { bundleRecording } from './site-recording-bundle.mjs';
import { SceneAssetSchema, SceneManifestSchema } from '../marketing/site/replay-model.ts';
import { measureWorkflowCamera } from './workflow-camera.mjs';
import { beforeSourceStoryboard, measureBeforeCamera, BEFORE_STEPS, BEFORE_CAMERA_TRANSITION_MS } from './before-source-storyboard.mjs';

assert(process.env.RECORDING_ATTACHMENTS && process.env.RECORDING_OUTPUT && process.env.RECORDING_SOURCE_DIST,
  'RECORDING_ATTACHMENTS, RECORDING_OUTPUT and RECORDING_SOURCE_DIST are required');
const attachments = resolve(process.env.RECORDING_ATTACHMENTS);
const output = resolve(process.env.RECORDING_OUTPUT);
const before = process.env.RECORDING_SCENE === 'before';
const scene = before ? 'before' : 'during';
const publicRoot = process.env.RECORDING_PUBLIC_PATH || (before ? '/media/before/' : '/media/workflow/');
const origin = process.env.RECORDING_URL || 'http://127.0.0.1:1443/';
await mkdir(attachments, { recursive: true });
const session = before ? 'before-paired-035' : 'workflow-paired-035';
function browser(...args) {
  const path = resolve(attachments, 'browser-response.json');
  const fd = openSync(path, 'w');
  try { execFileSync(process.env.AGENT_BROWSER_BIN || 'agent-browser', ['--session', session, '--json', ...args], { windowsHide: true, timeout: 30000, stdio: ['ignore', fd, 'inherit'] }); }
  catch (error) { throw new Error(`Browser command ${JSON.stringify(args)} failed: ${readFileSync(path, 'utf8')}`, { cause: error }); }
  finally { closeSync(fd); }
  const response = JSON.parse(readFileSync(path, 'utf8'));
  assert.equal(response.success, true, JSON.stringify(response));
  return response.data;
}
const evaluate = code => browser('eval', '-b', Buffer.from(code).toString('base64')).result;
const assets = [];
const mobileAssets = [];
try {
  for (const format of (process.env.RECORDING_FORMATS || 'desktop,mobile').split(','))
  for (const language of (process.env.SITE_LANGUAGES || 'zh,en').split(',')) for (const theme of (process.env.SITE_THEMES || 'dark,light').split(',')) {
    assert(['desktop', 'mobile'].includes(format));
    const width = format === 'mobile' ? 320 : 1440;
    const height = format === 'mobile' ? 780 : 900;
    const variant = `${language}-${theme}${format === 'mobile' ? '-mobile' : ''}`;
    const directory = resolve(attachments, variant);
    await mkdir(directory, { recursive: true });
    browser('open', `${origin}?scene=${scene}&language=${language}&theme=${theme}`);
    browser('set', 'viewport', String(width), String(height));
    browser('wait', '--fn', 'Boolean(window.goldBandPreview && document.querySelector("#workspace-center"))');
    evaluate('document.fonts.ready.then(()=>true)');
    const measurements = [];
    const cameraMeasurements = [];
    const recording = (before ? beforeSourceStoryboard : workflowSourceStoryboard)({ browser, evaluate, language, camera(stepId, { overview = false } = {}) {
      if (before) {
        cameraMeasurements.push({ stepId, overview, ...evaluate(`({timestamp:Date.now(),...(${measureBeforeCamera.toString()})(${JSON.stringify(overview ? 'overview' : stepId)})})`) });
        return;
      }
      const measurement = evaluate(`window.goldBandPreview.snapshot().then(snapshot => {
        const view = ${overview ? '{ width: innerWidth, height: innerHeight, desktop: { x: 0, y: 0, width: 1, height: 1 }, mobile: { x: 0, y: 0, width: 1, height: 1 } }' : `(${measureWorkflowCamera.toString()})(${JSON.stringify(stepId)}, snapshot.workflowGraph.edges, snapshot.workflowGraph.nodes)`};
        return { timestamp: Date.now(), ...view };
      })`);
      cameraMeasurements.push({ stepId, overview, ...measurement });
    }, screenshot(stepId) {
      const measurement = evaluate(before ? `(${measureBeforeCamera.toString()})(${JSON.stringify(stepId)})` : `window.goldBandPreview.snapshot().then(snapshot => (${measureWorkflowCamera.toString()})(${JSON.stringify(stepId)}, snapshot.workflowGraph.edges, snapshot.workflowGraph.nodes))`);
      assert.equal(measurement.width, width); assert.equal(measurement.height, height);
      measurements.push({ stepId, timestamp: evaluate('Date.now()'), ...measurement });
      browser('screenshot', resolve(directory, `${stepId}-${width}.png`));
    } });
    await writeFile(resolve(directory, 'source.json'), JSON.stringify(recording));
    await writeFile(resolve(directory, 'measurements.json'), JSON.stringify(measurements, null, 2));
    await writeFile(resolve(directory, 'camera-measurements.json'), JSON.stringify(cameraMeasurements, null, 2));
    const assetOutput = resolve(output, variant);
    const assetPath = `${publicRoot}${variant}/`;
    const bundle = await bundleRecording({ recording, captureOrigin: origin, sourceRoot: process.env.RECORDING_SOURCE_DIST, outputRoot: assetOutput, publicPath: assetPath });
    const markers = recording.events.filter(event => event.type === 5 && event.data.tag === 'semantic-checkpoint');
    assert.equal(markers.length, before ? BEFORE_STEPS.length : 11);
    const start = recording.events[0].timestamp;
    const checkpoints = markers.map((marker, index) => ({ stepId: marker.data.payload.stepId,
      startMs: index ? marker.timestamp - start : 0,
      endMs: markers[index + 1] ? markers[index + 1].timestamp - start : bundle.metadata.durationMs,
      poster: `${assetPath}${marker.data.payload.stepId}.png` }));
    for (const point of checkpoints) await copyFile(resolve(directory, `${point.stepId}-${width}.png`), resolve(assetOutput, `${point.stepId}.png`));
    const frames = track => cameraMeasurements.map((measurement, index) => ({
      timeMs: index ? measurement.timestamp - start : 0,
      rect: measurement[track], zoom: 1, transitionMs: index ? (before ? BEFORE_CAMERA_TRANSITION_MS : WORKFLOW_CAMERA_TRANSITION_MS) : 0,
    }));
    const asset = SceneAssetSchema.parse({ version: 1, scene, language, theme, rrwebVersion: '2.1.1', ...bundle.metadata,
      width, height, poster: checkpoints[0].poster, checkpoints,
      camera: { desktop: frames('desktop'), mobile: frames('mobile') },
      pace: checkpoints.map(point => ({ startMs: point.startMs, endMs: point.endMs, fromRate: point.stepId.startsWith('branch-') ? 1.5 : 1, toRate: 1 })),
    });
    (format === 'mobile' ? mobileAssets : assets).push(asset);
    await writeFile(resolve(assetOutput, 'asset.json'), JSON.stringify(asset, null, 2));
    console.log(JSON.stringify({ variant, ...bundle.metadata, resources: bundle.resources.length }));
  }
  await writeFile(resolve(output, 'manifest.json'), JSON.stringify(SceneManifestSchema.parse({ version: 1, assets, ...(mobileAssets.length ? { mobileAssets } : {}) }), null, 2));
} catch (error) {
  await writeFile(resolve(attachments, 'failed-snapshot.json'), JSON.stringify(browser('snapshot', '-i'), null, 2));
  browser('screenshot', resolve(attachments, 'failed.png'));
  throw error;
} finally { browser('close'); }
