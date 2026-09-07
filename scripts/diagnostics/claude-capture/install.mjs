import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { copyFile, mkdir, readFile, rename, writeFile } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const source = path.dirname(fileURLToPath(import.meta.url));
const home = path.join(os.homedir(), '.gold-band');
const target = path.join(home, 'diagnostics', 'claude-capture');
const settingsFile = path.join(home, 'settings.json');
const backupFile = path.join(target, 'activation-backup.json');
const mode = process.argv[2];
await mkdir(target, { recursive: true });
if (mode === 'prepare') {
  for (const name of ['package.json', 'package-lock.json', 'core.mjs', 'capture.mjs',
    'fixture.mjs', 'capture.test.mjs', 'verify.mjs', 'start-collector.ps1', 'install.mjs', 'README.md']) {
    if (source !== target) await copyFile(path.join(source, name), path.join(target, name));
  }
  const config = {
    adapterEntry: path.join(target, 'node_modules/@agentclientprotocol/claude-agent-acp/dist/index.js'),
    logsRoot: path.join(target, 'captures'), heartbeatMs: 60000,
    collectorStartup: path.join(target, 'start-collector.ps1'),
    telemetryEndpoint: 'http://127.0.0.1:14318/v1/logs',
    collectorHealth: 'http://127.0.0.1:14319/',
    ledger: { segmentBytes: 8 * 1024 * 1024, maxBytes: 512 * 1024 * 1024 },
    stderr: { segmentBytes: 8 * 1024 * 1024, maxBytes: 64 * 1024 * 1024 },
  };
  await writeFile(path.join(target, 'config.json'), JSON.stringify(config, null, 2));
  const collector = {
    receivers: { otlp: { protocols: { http: { endpoint: '127.0.0.1:14318' } } } },
    processors: { memory_limiter: { check_interval: '1s', limit_mib: 128, spike_limit_mib: 32 },
      batch: { timeout: '1s', send_batch_size: 128, send_batch_max_size: 256 } },
    exporters: { file: { path: path.join(target, 'telemetry', 'events.jsonl').replaceAll('\\', '/'),
      create_directory: true, rotation: { max_megabytes: 16, max_backups: 64, max_days: 14 } } },
    extensions: { health_check: { endpoint: '127.0.0.1:14319' } },
    service: { extensions: ['health_check'], telemetry: { logs: { level: 'warn' }, metrics: { level: 'none' } },
      pipelines: { logs: { receivers: ['otlp'], processors: ['memory_limiter', 'batch'], exporters: ['file'] } } },
  };
  await writeFile(path.join(target, 'collector.yaml'), JSON.stringify(collector, null, 2));
  console.log(JSON.stringify({ prepared: target, next: 'Install dependencies and collector, then validate before enable.' }));
} else if (mode === 'enable' || mode === 'disable') {
  const original = await readFile(settingsFile, 'utf8');
  const settings = JSON.parse(original);
  const adapter = settings.agents?.['claude-acp']?.adapter;
  assert(adapter, 'Claude agent settings missing');
  const wrapped = adapter.command === process.execPath && adapter.args?.[0] === path.join(target, 'capture.mjs');
  if (mode === 'enable') {
    const manifest = JSON.parse(await readFile(path.join(target, 'package.json'), 'utf8'));
    const adapterPackage = '@agentclientprotocol/claude-agent-acp';
    const packageSpec = `${adapterPackage}@${manifest.dependencies[adapterPackage]}`;
    assert(wrapped || (adapter.command === 'npx' && adapter.args?.includes(packageSpec)),
      'Unexpected launch configuration; refusing to replace it');
    const validation = JSON.parse(await readFile(path.join(target, 'validation', 'verification.json'), 'utf8'));
    assert(validation.passed && validation.telemetryVerified, 'Real adapter and telemetry validation required');
    if (!wrapped) {
      const existing = await readFile(backupFile, 'utf8').then(JSON.parse).catch(error => {
        if (error.code === 'ENOENT') return null;
        throw error;
      });
      if (existing) assert.deepEqual(existing.adapter, adapter, 'Backup differs from current adapter');
      else await writeFile(backupFile, JSON.stringify({ adapter, logLevel: settings.logLevel,
        savedAt: new Date().toISOString() }, null, 2), { flag: 'wx' });
    }
    settings.agents['claude-acp'].adapter = { ...adapter, command: process.execPath,
      args: [path.join(target, 'capture.mjs'), path.join(target, 'config.json')] };
    settings.logLevel = 'debug';
  } else {
    assert(wrapped, 'Launch configuration changed since activation; refusing to overwrite');
    const backup = JSON.parse(await readFile(backupFile, 'utf8'));
    settings.agents['claude-acp'].adapter = backup.adapter;
    if (settings.logLevel === 'debug') settings.logLevel = backup.logLevel;
  }
  const tmp = `${settingsFile}.capture-${process.pid}.tmp`;
  await writeFile(tmp, JSON.stringify(settings, null, 2) + '\n', { flag: 'wx' });
  assert.equal(await readFile(settingsFile, 'utf8'), original, 'Settings changed concurrently; not replaced');
  await rename(tmp, settingsFile);
  const startup = `powershell.exe -NoProfile -WindowStyle Hidden -ExecutionPolicy Bypass -File "${path.join(target, 'start-collector.ps1')}"`;
  const args = mode === 'enable'
    ? ['add', 'HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run', '/v', 'GoldBandClaudeCapture', '/t', 'REG_SZ', '/d', startup, '/f']
    : ['delete', 'HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run', '/v', 'GoldBandClaudeCapture', '/f'];
  const registry = spawnSync('reg.exe', args, { windowsHide: true, stdio: 'pipe' });
  assert.equal(registry.status, 0, 'Settings changed, but collector login startup could not be updated');
  console.log(JSON.stringify({ mode, settingsFile, target, restartGoldBandRequired: true }));
} else {
  throw new Error('Usage: node install.mjs prepare|enable|disable');
}
