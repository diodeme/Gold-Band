import assert from 'node:assert/strict';
import test from 'node:test';

import { BUILTIN_AGENT_IDS, buildAgentCatalog } from './prepare-agent-catalog.mjs';

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
