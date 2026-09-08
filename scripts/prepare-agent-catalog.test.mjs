import assert from 'node:assert/strict';
import test from 'node:test';
import { readFile } from 'node:fs/promises';

import { BUILTIN_AGENT_IDS, buildAgentCatalog } from './prepare-agent-catalog.mjs';

function registryFixture() {
  return { agents: BUILTIN_AGENT_IDS.map((id) => ({
    id, version: '0.73.0',
    distribution: { npx: { package: `@example/${id}@0.73.0`, args: ['--acp'] } },
  })) };
}

test('local pins override both executable version and metadata without changing registry', () => {
  const registry = registryFixture();
  const before = structuredClone(registry);
  const catalog = buildAgentCatalog(registry, 'fixed', { versionPins: { 'claude-acp': '0.72.0' } });
  const claude = catalog.agents.find((agent) => agent.id === 'claude-acp');
  assert.equal(claude.version, '0.72.0');
  assert.deepEqual(claude.args, ['-y', '@example/claude-acp@0.72.0', '--acp']);
  assert.equal(catalog.agents.find((agent) => agent.id === 'codex-acp').version, '0.73.0');
  assert.deepEqual(registry, before);
});

test('invalid pins fail explicitly instead of silently following registry', () => {
  for (const version of [-1, '', 'latest', '^0.72.0', '0.72', null]) {
    assert.throws(() => buildAgentCatalog(registryFixture(), 'fixed', {
      versionPins: { 'claude-acp': version },
    }), /version/i);
  }
  assert.throws(() => buildAgentCatalog(registryFixture(), 'fixed', {
    versionPins: { typo: '0.72.0' },
  }), /unknown/i);
  assert.throws(() => buildAgentCatalog(registryFixture(), 'fixed', {
    versionPins: { cursor: '0.72.0' },
  }), /npx/i);
});

test('pins support unscoped packages and preserve arguments and environment', () => {
  const registry = registryFixture();
  const agent = registry.agents.find((entry) => entry.id === 'pi-acp');
  agent.distribution.npx = { package: 'pi-acp@latest', args: ['--acp'], env: { EXAMPLE: 'value' } };
  const result = buildAgentCatalog(registry, 'fixed', { versionPins: { 'pi-acp': '1.0.0-beta.1' } })
    .agents.find((entry) => entry.id === 'pi-acp');
  assert.deepEqual(result.args, ['-y', 'pi-acp@1.0.0-beta.1', '--acp']);
  assert.deepEqual(result.env, { EXAMPLE: 'value' });
});

test('policy rejects malformed maps and non-registry distributions', () => {
  for (const policy of [null, [], { versionPin: {} }, { versionPins: null }, { versionPins: [] }]) {
    assert.throws(() => buildAgentCatalog(registryFixture(), 'fixed', policy));
  }
  const registry = registryFixture();
  registry.agents[0].distribution = {};
  assert.throws(() => buildAgentCatalog(registry, 'fixed', { versionPins: { 'claude-acp': '0.72.0' } }), /npx/);
  registry.agents[0].distribution = { npx: { package: 'https://example.com/agent.tgz' } };
  assert.throws(() => buildAgentCatalog(registry, 'fixed', { versionPins: { 'claude-acp': '0.72.0' } }), /registry package/);
});

test('checked-in policy overrides the snapshot and future online versions', async () => {
  const policy = JSON.parse(await readFile(new URL('../configs/agent-catalog-policy.json', import.meta.url), 'utf8'));
  const registry = JSON.parse(await readFile(new URL('../resources/acp-registry.snapshot.json', import.meta.url), 'utf8'));
  for (const version of ['0.73.0', '0.75.1']) {
    const claude = registry.agents.find((entry) => entry.id === 'claude-acp');
    claude.version = version;
    claude.distribution.npx.package = `@agentclientprotocol/claude-agent-acp@${version}`;
    const result = buildAgentCatalog(registry, 'fixed', policy).agents.find((entry) => entry.id === 'claude-acp');
    assert.equal(result.version, '0.72.0');
    assert.equal(result.args[1], '@agentclientprotocol/claude-agent-acp@0.72.0');
  }
});

test('filters the official registry to the curated catalog without GLM', () => {
  const agents = BUILTIN_AGENT_IDS.map((id) => ({
    id,
    name: id,
    version: '1.0.0',
    description: id,
    icon: `https://example.com/${id}.svg`,
    distribution: { npx: { package: `${id}@1.0.0`, args: ['--acp'] } },
  }));
  const catalog = buildAgentCatalog({ version: '1.0.0', agents }, '2026-08-07T00:00:00.000Z');

  assert.deepEqual(catalog.agents.map((agent) => agent.id), BUILTIN_AGENT_IDS);
  assert.equal(catalog.agents.some((agent) => agent.id.includes('glm')), false);
  assert.equal(catalog.agents.find((agent) => agent.id === 'amp-acp').primaryAgentDir, '.agents');
  assert.deepEqual(catalog.agents.find((agent) => agent.id === 'amp-acp').compatibleAgentDirs, ['.claude']);
  assert.equal(catalog.agents.find((agent) => agent.id === 'claude-acp').supportsSystemPrompt, true);
  const kimi = catalog.agents.find((agent) => agent.id === 'kimi');
  assert.equal(kimi.primaryAgentDir, '.kimi-code');
  assert.deepEqual(kimi.compatibleAgentDirs, ['.agents']);
  assert.equal(kimi.supportsSystemPrompt, false);
  const pi = catalog.agents.find((agent) => agent.id === 'pi-acp');
  assert.equal(pi.command, 'npx');
  assert.deepEqual(pi.args, ['-y', 'pi-acp@1.0.0', '--acp']);
  assert.equal(pi.primaryAgentDir, '.pi/agent');
  assert.equal(pi.projectPrimaryAgentDir, '.pi');
  assert.deepEqual(pi.compatibleAgentDirs, ['.agents']);
  assert.equal(pi.supportsSystemPrompt, false);
  assert.equal(pi.supportsExternalSessionSync, false);
});

test('fails rather than silently publishing an incomplete catalog', () => {
  assert.throws(
    () => buildAgentCatalog({ version: '1.0.0', agents: [] }),
    /missing required agents/,
  );
});
