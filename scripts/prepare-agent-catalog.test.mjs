import assert from 'node:assert/strict';
import test from 'node:test';
import { createHash } from 'node:crypto';
import { mkdir, mkdtemp, readFile, readdir, rename, rm, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { tmpdir } from 'node:os';

import {
  ACP_REGISTRY_URL, AgentCatalogFetchError, BUILTIN_AGENT_IDS, SimulatedPublicationCrash,
  buildAgentCatalog, generateAgentCatalog,
  recoverAgentCatalogPublication,
} from './prepare-agent-catalog.mjs';

function registryFixture(version = '0.73.0') {
  return { version, agents: BUILTIN_AGENT_IDS.map((id) => ({
    id, version,
    name: id,
    description: id,
    icon: `https://icons.example/${id}.svg`,
    distribution: { npx: { package: `@example/${id}@${version}`, args: ['--acp'] } },
  })) };
}

function officialPolicy(extra = {}) {
  return {
    ...extra,
    validatedVersions: Object.fromEntries(BUILTIN_AGENT_IDS.map((id) => [id, '0.73.0'])),
  };
}

test('local pins override both executable version and metadata without changing registry', () => {
  const registry = registryFixture();
  const before = structuredClone(registry);
  const catalog = buildAgentCatalog(registry, 'fixed', officialPolicy({
    versionPins: { 'claude-acp': '0.72.0' },
  }));
  const claude = catalog.agents.find((agent) => agent.id === 'claude-acp');
  assert.equal(claude.version, '0.72.0');
  assert.deepEqual(claude.provenance, {
    sourceKind: 'official-registry',
    integrationOrigin: 'official',
    validatedVersion: '0.73.0',
  });
  assert.deepEqual(claude.args, ['-y', '@example/claude-acp@0.72.0', '--acp']);
  assert.equal(catalog.agents.find((agent) => agent.id === 'codex-acp').version, '0.73.0');
  assert.deepEqual(registry, before);
});

test('invalid pins fail explicitly instead of silently following registry', () => {
  for (const version of [-1, '', 'latest', '^0.72.0', '0.72', null]) {
    assert.throws(() => buildAgentCatalog(registryFixture(), 'fixed', officialPolicy({
      versionPins: { 'claude-acp': version },
    })), /version/i);
  }
  assert.throws(() => buildAgentCatalog(registryFixture(), 'fixed', officialPolicy({
    versionPins: { typo: '0.72.0' },
  })), /unknown/i);
  assert.throws(() => buildAgentCatalog(registryFixture(), 'fixed', officialPolicy({
    versionPins: { cursor: '0.72.0' },
  })), /npx/i);
});

test('pins support unscoped packages and preserve arguments and environment', () => {
  const registry = registryFixture();
  const agent = registry.agents.find((entry) => entry.id === 'pi-acp');
  agent.distribution.npx = { package: 'pi-acp@latest', args: ['--acp'], env: { EXAMPLE: 'value' } };
  const result = buildAgentCatalog(registry, 'fixed', officialPolicy({
    versionPins: { 'pi-acp': '1.0.0-beta.1' },
  }))
    .agents.find((entry) => entry.id === 'pi-acp');
  assert.deepEqual(result.args, ['-y', 'pi-acp@1.0.0-beta.1', '--acp']);
  assert.deepEqual(result.env, { EXAMPLE: 'value' });
});

test('policy rejects malformed maps and non-registry distributions', () => {
  for (const policy of [null, [], { versionPin: {} }, { versionPins: null }, { versionPins: [] }]) {
      const candidate = policy && !Array.isArray(policy) ? officialPolicy(policy) : policy;
      assert.throws(() => buildAgentCatalog(registryFixture(), 'fixed', candidate));
  }
  const registry = registryFixture();
  registry.agents[0].distribution = {};
  assert.throws(() => buildAgentCatalog(registry, 'fixed', officialPolicy({
    versionPins: { 'claude-acp': '0.72.0' },
  })), /npx/);
  registry.agents[0].distribution = { npx: { package: 'https://example.com/agent.tgz' } };
  assert.throws(() => buildAgentCatalog(registry, 'fixed', officialPolicy({
    versionPins: { 'claude-acp': '0.72.0' },
  })), /registry package/);
});

test('launch overrides append adapter arguments without changing registry source', () => {
  const registry = registryFixture();
  const before = structuredClone(registry);
  const catalog = buildAgentCatalog(registry, 'fixed', officialPolicy({
    launchOverrides: {
      'claude-acp': {
        extraArgs: ['--settings', '{"security":{"auth":{"selectedType":"qoder-browser"}}}'],
        localCommand: 'claude-local',
        installedCommand: 'claude-local',
      },
    },
  }));
  const claude = catalog.agents.find((agent) => agent.id === 'claude-acp');
  assert.deepEqual(claude.args, [
    '-y', '@example/claude-acp@0.73.0', '--acp',
    '--settings', '{"security":{"auth":{"selectedType":"qoder-browser"}}}',
  ]);
  assert.deepEqual(claude.localLaunch, {
    command: 'claude-local',
    args: ['--acp', '--settings', '{"security":{"auth":{"selectedType":"qoder-browser"}}}'],
  });
  assert.equal(claude.installedCommand, 'claude-local');
  assert.deepEqual(registry, before);
  assert.throws(() => buildAgentCatalog(registry, 'fixed', officialPolicy({
    launchOverrides: { unknown: { extraArgs: ['--x'] } },
  })), /unknown agent launch override/i);
  assert.throws(() => buildAgentCatalog(registry, 'fixed', officialPolicy({
    launchOverrides: { 'claude-acp': { command: 'unsafe' } },
  })), /unsupported field/i);
});

test('checked-in policy applies to the snapshot and future online versions', async () => {
  const policy = JSON.parse(await readFile(new URL('../configs/agent-catalog-policy.json', import.meta.url), 'utf8'));
  assert.deepEqual(policy.versionPins, {
    'claude-acp': '0.90.0',
    'codex-acp': '2.1.1',
    qoder: '1.1.86',
  });
  assert.equal(policy.validatedVersions.gemini, '0.66.0');
  assert.equal(policy.validatedVersions['qwen-code'], '0.26.6');
  assert.equal(policy.validatedVersions.qoder, '1.1.86');
  assert.deepEqual(policy.launchOverrides, {
    qoder: { localCommand: 'qodercli', installedCommand: 'qodercli' },
  });
  const localRaw = await readFile(new URL('../resources/acp-registry-local.json', import.meta.url), 'utf8');
  const localSupplement = JSON.parse(localRaw);
  const registry = JSON.parse(await readFile(new URL('../resources/acp-registry.snapshot.json', import.meta.url), 'utf8'));
  for (const version of ['0.73.0', '0.75.1']) {
    const claude = registry.agents.find((entry) => entry.id === 'claude-acp');
    claude.version = version;
    claude.distribution.npx.package = `@agentclientprotocol/claude-agent-acp@${version}`;
    const codex = registry.agents.find((entry) => entry.id === 'codex-acp');
    codex.version = version;
    codex.distribution.npx.package = `@agentclientprotocol/codex-acp@${version}`;
    const catalog = buildAgentCatalog(
      registry,
      'fixed',
      policy,
      localSupplement,
      createHash('sha256').update(localRaw).digest('hex'),
    );
    for (const agent of registry.agents.filter((entry) => BUILTIN_AGENT_IDS.includes(entry.id))) {
      const result = catalog.agents.find((entry) => entry.id === agent.id);
      const expected = policy.versionPins?.[agent.id] ?? agent.version;
      assert.equal(result.version, expected, agent.id);
      if (agent.distribution?.npx && result.command === 'npx') {
        assert.ok(result.args[1].endsWith(`@${expected}`), `${agent.id}: ${result.args[1]}`);
      }
    }
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
  const catalog = buildAgentCatalog({ version: '1.0.0', agents }, '2026-08-07T00:00:00.000Z', officialPolicy());

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
  assert.equal(catalog.schemaVersion, 2);
  const claude = catalog.agents.find((agent) => agent.id === 'claude-acp');
  assert.equal(claude.version, '1.0.0');
  assert.equal(claude.provenance.validatedVersion, '0.73.0');
  assert.equal(catalog.source.officialRegistry.url, 'https://cdn.agentclientprotocol.com/registry/v1/latest/registry.json');
  assert.match(catalog.source.localSupplement.sha256, /^[a-f0-9]{64}$/);
  assert.deepEqual(
    catalog.agents.map((agent) => agent.excludedPlatforms),
    BUILTIN_AGENT_IDS.map((id) => (id === 'qoder' ? [{ os: 'windows', arch: 'aarch64' }] : [])),
  );
});

test('checked-in first-stage expansion pins Qoder and curates DeepSeek locally', async () => {
  const policy = JSON.parse(await readFile(new URL('../configs/agent-catalog-policy.json', import.meta.url), 'utf8'));
  const localRaw = await readFile(new URL('../resources/acp-registry-local.json', import.meta.url), 'utf8');
  const registry = JSON.parse(await readFile(new URL('../resources/acp-registry.snapshot.json', import.meta.url), 'utf8'));
  const qoderRegistry = registry.agents.find((entry) => entry.id === 'qoder');
  const registryQoderVersion = String(qoderRegistry.version);
  qoderRegistry.version = '9.9.9';
  qoderRegistry.distribution.npx.package = '@qoder-ai/qodercli@9.9.9';
  const catalog = buildAgentCatalog(
    registry,
    'fixed',
    policy,
    JSON.parse(localRaw),
    createHash('sha256').update(localRaw).digest('hex'),
  );

  const qoder = catalog.agents.find((agent) => agent.id === 'qoder');
  assert.equal(qoder.version, '1.1.86');
  assert.notEqual(registryQoderVersion, '9.9.9');
  assert.deepEqual(qoder.provenance, {
    sourceKind: 'official-registry',
    integrationOrigin: 'official',
    validatedVersion: '1.1.86',
  });
  assert.deepEqual(qoder.args, ['-y', '@qoder-ai/qodercli@1.1.86', '--acp']);
  assert.deepEqual(qoder.localLaunch, { command: 'qodercli', args: ['--acp'] });
  assert.equal(qoder.installedCommand, 'qodercli');
  assert.equal(qoder.primaryAgentDir, '.qoder');
  assert.deepEqual(qoder.compatibleAgentDirs, ['.agents']);
  assert.deepEqual(qoder.excludedPlatforms, [{ os: 'windows', arch: 'aarch64' }]);

  const deepseek = catalog.agents.find((agent) => agent.id === 'deepseek-harness');
  assert.deepEqual(deepseek.provenance, {
    sourceKind: 'local-supplement',
    integrationOrigin: 'official',
    validatedVersion: '0.2.1-rc.2',
  });
  assert.equal(deepseek.version, '0.2.1-rc.2');
  assert.equal(deepseek.command, 'dsh');
  assert.deepEqual(deepseek.args, ['--profile', 'acp']);
  assert.equal(deepseek.localLaunch, null);
  assert.equal(deepseek.installedCommand, null);
  assert.equal(deepseek.primaryAgentDir, '.dsh');
  assert.deepEqual(deepseek.compatibleAgentDirs, ['.agents']);
  assert.deepEqual(deepseek.excludedPlatforms, []);

  const zcode = catalog.agents.find((agent) => agent.id === 'zcode-acp');
  assert.deepEqual(zcode.provenance, {
    sourceKind: 'local-supplement',
    integrationOrigin: 'community-adapter',
    validatedVersion: '0.84.1',
  });
  assert.equal(zcode.version, '0.84.1');
  assert.equal(zcode.command, 'zcode-acp-server');
  assert.deepEqual(zcode.args, []);
  assert.equal(zcode.localLaunch, null);
  assert.equal(zcode.installedCommand, null);
  assert.equal(zcode.primaryAgentDir, '.zcode');
  assert.equal(zcode.projectPrimaryAgentDir, '.agents');
  assert.deepEqual(zcode.compatibleAgentDirs, ['.agents']);
  assert.deepEqual(zcode.excludedPlatforms, []);
});

test('official registry versions cannot silently become validation evidence', () => {
  const invalid = officialPolicy();
  invalid.validatedVersions['claude-acp'] = null;
  assert.throws(
    () => buildAgentCatalog(registryFixture(), 'fixed', invalid),
    /validated version/,
  );
});

test('every official catalog entry requires explicit validation evidence', () => {
  for (const id of BUILTIN_AGENT_IDS) {
    const policy = officialPolicy();
    delete policy.validatedVersions[id];
    assert.throws(
      () => buildAgentCatalog(registryFixture(), 'fixed', policy),
      /validated version/i,
      id,
    );
  }
});

test('validated versions must match the Rust semantic version contract', () => {
  for (const version of ['', null, 73, 'latest', '^0.73.0', 'v0.73.0', ' 0.73.0 ']) {
    const policy = officialPolicy();
    policy.validatedVersions.qoder = version;
    assert.throws(
      () => buildAgentCatalog(registryFixture(), 'fixed', policy),
      /validated version/i,
      String(version),
    );
  }
  const policy = officialPolicy();
  policy.validatedVersions.qoder = '0.73.0-rc.1+validated.2';
  assert.equal(
    buildAgentCatalog(registryFixture(), 'fixed', policy).agents.find((agent) => agent.id === 'qoder')
      .provenance.validatedVersion,
    '0.73.0-rc.1+validated.2',
  );
});

test('local supplement entries carry provenance and typed platform exclusions', async (t) => {
  const registry = registryFixture();
  registry.agents = registry.agents.filter((agent) => agent.id !== 'pi-acp');
  const local = {
    agents: [{
      id: 'pi-acp',
      label: 'Local Pi',
      version: '0.2.1-rc.2',
      provenance: {
        sourceKind: 'local-supplement',
        integrationOrigin: 'official',
        validatedVersion: '0.2.1-rc.2',
      },
      description: 'fixture',
      iconUrl: 'https://icons.example/deepseek-harness.svg',
      command: 'pi-acp-local',
      args: ['--acp'],
      primaryAgentDir: '.pi/agent',
      compatibleAgentDirs: ['.agents'],
    }],
  };
  const policy = officialPolicy();
  delete policy.validatedVersions['pi-acp'];
  const digest = createHash('sha256').update(JSON.stringify(local)).digest('hex');
  const catalog = buildAgentCatalog(registry, 'fixed', policy, local, digest);
  const agent = catalog.agents.find((entry) => entry.id === 'pi-acp');

  assert.equal(agent.provenance.sourceKind, 'local-supplement');
  assert.equal(agent.provenance.validatedVersion, '0.2.1-rc.2');
  assert.equal(catalog.source.localSupplement.sha256, digest);
  assert.throws(() => buildAgentCatalog(registry, 'fixed', policy, {
    agents: [{ ...local.agents[0], excludedPlatforms: [{ os: 'win32', arch: 'arm64' }] }],
  }, digest), /invalid platform selector/);
  assert.throws(() => buildAgentCatalog(registryFixture(), 'fixed', officialPolicy(), {
    agents: [{ ...local.agents[0], id: 'claude-acp' }],
  }, digest), /both official Registry and local supplement/);

  await t.test('rejects validation evidence in both the policy and local supplement', () => {
    assert.throws(
      () => buildAgentCatalog(registry, 'fixed', officialPolicy(), local, digest),
      /local.*validated version|validated version.*local/i,
    );
  });
  for (const version of [undefined, null, '', 73, 'latest', 'v0.2.1', ' 0.2.1 ']) {
    await t.test(`rejects local validated version ${String(version)}`, () => {
      const invalid = structuredClone(local);
      invalid.agents[0].version = version;
      invalid.agents[0].provenance.validatedVersion = version;
      assert.throws(
        () => buildAgentCatalog(registry, 'fixed', policy, invalid, digest),
        /provenance|validated version/i,
      );
    });
  }
});

test('fails rather than silently publishing an incomplete catalog', () => {
  assert.throws(
    () => buildAgentCatalog({ version: '1.0.0', agents: [] }),
    /missing required agents/,
  );
});

test('catalog describes installed ACP commands independently of npm launchers', () => {
  const catalog = buildAgentCatalog(registryFixture(), 'fixed', officialPolicy());
  for (const [id, command] of [
    ['claude-acp', 'claude-agent-acp'], ['codex-acp', 'codex-acp'],
    ['gemini', 'gemini'], ['codebuddy-code', 'codebuddy'],
    ['qwen-code', 'qwen'], ['pi-acp', 'pi-acp'],
  ]) {
    const agent = catalog.agents.find((entry) => entry.id === id);
    assert.deepEqual(agent.localLaunch, { command, args: ['--acp'] });
  }
  assert.equal(catalog.agents.find((entry) => entry.id === 'claude-acp').installedCommand, 'claude');
  assert.equal(catalog.agents.find((entry) => entry.id === 'codex-acp').installedCommand, 'codex');
  assert.equal(catalog.agents.find((entry) => entry.id === 'pi-acp').installedCommand, 'pi');
  assert.equal(catalog.agents.find((entry) => entry.id === 'cursor').localLaunch, null);
});

test('retries a transient registry request before publishing the catalog', async () => {
  const root = await createGenerationRoot();
  let registryAttempts = 0;
  try {
    const registry = registryFixture();
    const catalog = await generateAgentCatalog({
      repoRoot: root,
      retrySleep: async () => {},
      fetchJson: async (url) => {
        if (url.endsWith('/registry.json') && registryAttempts++ === 0) {
          throw new TypeError('fetch failed');
        }
        if (url.endsWith('/registry.json')) return registry;
        return { version: decodeURIComponent(url.split('/').pop()) };
      },
      fetchIcon: async (url) => `<svg xmlns="https://www.w3.org/2000/svg">${url}</svg>`,
    });

    assert.equal(catalog.agents.length, BUILTIN_AGENT_IDS.length);
    assert.equal(registryAttempts, 2);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test('exhausted catalog requests identify the stage, URL and attempt count', async () => {
  const root = await createGenerationRoot();
  let registryAttempts = 0;
  try {
    await assert.rejects(
      () => generateAgentCatalog({
        repoRoot: root,
        retrySleep: async () => {},
        fetchJson: async () => {
          registryAttempts += 1;
          throw new TypeError('fetch failed');
        },
      }),
      (error) => {
        assert.ok(error instanceof AgentCatalogFetchError);
        assert.equal(error.code, 'AGENT_CATALOG_FETCH_FAILED');
        assert.equal(error.stage, 'registry');
        assert.equal(error.url, ACP_REGISTRY_URL);
        assert.equal(error.attempts, 3);
        assert.match(error.message, /fetch failed/);
        return true;
      },
    );
    assert.equal(registryAttempts, 3);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

async function createGenerationRoot(policy = officialPolicy()) {
  const root = await mkdtemp(join(tmpdir(), 'gold-band-agent-catalog-test-'));
  await mkdir(join(root, 'resources'), { recursive: true });
  await mkdir(join(root, 'configs'), { recursive: true });
  await writeFile(join(root, 'configs', 'agent-catalog-policy.json'), JSON.stringify(policy));
  await writeFile(join(root, 'resources', 'acp-registry-local.json'), '{"agents":[]}');
  return root;
}

function generationFetchers(registry = registryFixture()) {
  return {
    fetchJson: async (url) => {
      if (url.endsWith('/registry.json')) return registry;
      const version = decodeURIComponent(url.split('/').pop());
      return { version };
    },
    fetchIcon: async (url) => `<svg xmlns="https://www.w3.org/2000/svg">${registry.version} ${url}</svg>`,
  };
}

async function officialState(root) {
  const iconDir = join(root, 'web', 'public', 'agent-icons');
  const readOptional = async (path) => readFile(path, 'utf8').catch((error) => {
    if (error.code === 'ENOENT') return null;
    throw error;
  });
  const iconNames = (await readdir(iconDir).catch((error) => {
    if (error.code === 'ENOENT') return [];
    throw error;
  })).filter((name) => name.endsWith('.svg')).sort();
  return {
    snapshot: await readOptional(join(root, 'resources', 'acp-registry.snapshot.json')),
    catalog: await readOptional(join(root, 'resources', 'agent-catalog.json')),
    icons: await Promise.all(iconNames.map(async (name) => [
      name,
      await readFile(join(iconDir, name), 'utf8'),
    ])),
  };
}

async function initializeGenerationRoot(policy = officialPolicy()) {
  const root = await createGenerationRoot(policy);
  await generateAgentCatalog({ repoRoot: root, ...generationFetchers() });
  return root;
}

test('catalog publication removes only icons managed by the previous catalog', async () => {
  const root = await initializeGenerationRoot();
  try {
    const stale = join(root, 'web', 'public', 'agent-icons', 'stale.svg');
    await writeFile(stale, '<svg>stale</svg>');
    const catalogPath = join(root, 'resources', 'agent-catalog.json');
    const previousCatalog = JSON.parse(await readFile(catalogPath, 'utf8'));
    const claude = previousCatalog.agents.find((agent) => agent.id === 'claude-acp');
    claude.iconKey = 'claude-legacy';
    await writeFile(catalogPath, `${JSON.stringify(previousCatalog, null, 2)}\n`);
    await rename(join(root, 'web', 'public', 'agent-icons', 'claude.svg'), join(root, 'web', 'public', 'agent-icons', 'claude-legacy.svg'));
    await generateAgentCatalog({ repoRoot: root, ...generationFetchers() });
    assert.equal((await readdir(join(root, 'web', 'public', 'agent-icons'))).includes('stale.svg'), true);
    assert.equal((await readdir(join(root, 'web', 'public', 'agent-icons'))).includes('claude-legacy.svg'), false);
    assert.equal((await officialState(root)).icons.length, BUILTIN_AGENT_IDS.length + 1);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test('ordinary catalog generation failures leave official outputs byte-for-byte unchanged', async () => {
  const scenarios = [
    ['registry-fetch', { fetchJson: async () => { throw new Error('offline'); } }],
    ['npm-pin-validation', { hooks: { beforeNpmPinValidation: () => { throw new Error('pin unavailable'); } } }],
    ['icon-download', { fetchIcon: async () => { throw new Error('icon unavailable'); } }],
    ['icon-html-response', { fetchIcon: async () => '<!DOCTYPE html><svg xmlns="https://www.w3.org/2000/svg"></svg>' }],
    ['icon-validation', { hooks: { beforeIconValidation: () => { throw new Error('invalid icon'); } } }],
    ['catalog-validation', { hooks: { beforeCatalogValidation: () => { throw new Error('invalid catalog'); } } }],
    ['official-replace', { hooks: { beforeReplace: () => { throw new Error('replace failed'); } } }],
  ];
  for (const [name, options] of scenarios) {
    const policy = name === 'npm-pin-validation' ? officialPolicy({
      versionPins: { 'claude-acp': '0.72.0' },
    }) : officialPolicy();
    const root = await initializeGenerationRoot(policy);
    try {
      const before = await officialState(root);
      await assert.rejects(
        () => generateAgentCatalog({ repoRoot: root, ...generationFetchers(), ...options }),
        /offline|unavailable|failed|invalid|svg/i,
        name,
      );
      assert.deepEqual(await officialState(root), before, name);
    } finally {
      await rm(root, { recursive: true, force: true });
    }
  }
});

const PUBLICATION_JOURNAL = 'resources/.agent-catalog-publish-journal.json';
const PUBLICATION_TARGET_COUNT = BUILTIN_AGENT_IDS.length + 3;

async function initializePublicationRoot() {
  const root = await initializeGenerationRoot();
  const catalogPath = join(root, 'resources', 'agent-catalog.json');
  const catalog = JSON.parse(await readFile(catalogPath, 'utf8'));
  catalog.agents.find((agent) => agent.id === 'claude-acp').iconKey = 'claude-legacy';
  await writeFile(catalogPath, `${JSON.stringify(catalog, null, 2)}\n`);
  const icons = join(root, 'web', 'public', 'agent-icons');
  await rename(join(icons, 'claude.svg'), join(icons, 'claude-legacy.svg'));
  await writeFile(join(icons, 'unmanaged.svg'), '<svg>unmanaged</svg>');
  return root;
}

async function publicationEvidence(root) {
  const journal = await readFile(join(root, PUBLICATION_JOURNAL), 'utf8');
  const artifacts = {};
  for (const record of JSON.parse(journal).records) {
    for (const path of [record.temp, record.backup].filter(Boolean)) {
      artifacts[path] = await readFile(join(root, path), 'utf8').catch((error) => {
        if (error.code === 'ENOENT') return null;
        throw error;
      });
    }
  }
  return { journal, artifacts };
}

test('partial publication failures and crashes recover the entire previous set', async (t) => {
  for (const crash of [false, true]) {
    const positions = crash ? [1, 2, 3, PUBLICATION_TARGET_COUNT - 1] : [1, 2, 3, PUBLICATION_TARGET_COUNT];
    for (const position of positions) {
      await t.test(`${crash ? 'crash' : 'error'} after replacement ${position}`, async () => {
        const root = await initializePublicationRoot();
        try {
          const before = await officialState(root);
          let replacements = 0;
          await assert.rejects(
            () => generateAgentCatalog({
              repoRoot: root,
              ...generationFetchers(registryFixture('0.74.0')),
              hooks: {
                afterReplace: () => {
                  if (++replacements === position) {
                    throw crash ? new SimulatedPublicationCrash() : new Error('replace failed');
                  }
                },
              },
            }),
            crash ? SimulatedPublicationCrash : /replace failed/,
          );
          if (crash) {
            assert.notEqual((await officialState(root)).snapshot, before.snapshot);
            assert.equal(await recoverAgentCatalogPublication(root), true);
          }
          assert.deepEqual(await officialState(root), before);
          assert.equal(await recoverAgentCatalogPublication(root), false, 'recovery is idempotent');
          for (const path of ['resources', 'web/public/agent-icons']) {
            assert.equal((await readdir(join(root, path))).some((name) => name.includes('.catalog-')), false);
          }
        } finally {
          await rm(root, { recursive: true, force: true });
        }
      });
    }
  }
});

test('a fully replaced publication confirms the complete new set without requiring backups', async () => {
  const root = await initializePublicationRoot();
  try {
    const before = await officialState(root);
    let replacements = 0;
    await assert.rejects(() => generateAgentCatalog({
      repoRoot: root,
      ...generationFetchers(registryFixture('0.74.0')),
      hooks: {
        afterReplace: () => {
          if (++replacements === PUBLICATION_TARGET_COUNT) throw new SimulatedPublicationCrash();
        },
      },
    }), SimulatedPublicationCrash);
    const afterCrash = await officialState(root);
    assert.notEqual(afterCrash.snapshot, before.snapshot);
    assert.notEqual(afterCrash.catalog, before.catalog);
    assert.equal(JSON.parse(afterCrash.snapshot).version, '0.74.0');
    assert.equal(afterCrash.icons.some(([name]) => name === 'claude-legacy.svg'), false);
    for (const [name, content] of afterCrash.icons) {
      assert.ok(name === 'unmanaged.svg' || content.includes('0.74.0'), name);
    }
    const journal = JSON.parse(await readFile(join(root, PUBLICATION_JOURNAL), 'utf8'));
    for (const record of journal.records) {
      if (record.backup) await rm(join(root, record.backup), { force: true });
    }
    assert.equal(await recoverAgentCatalogPublication(root), true);
    assert.deepEqual(await officialState(root), afterCrash);
    assert.equal(await recoverAgentCatalogPublication(root), false);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test('interrupted first publication restores absent files', async () => {
  const root = await createGenerationRoot();
  try {
    const before = await officialState(root);
    let replacements = 0;
    await assert.rejects(() => generateAgentCatalog({
      repoRoot: root,
      ...generationFetchers(),
      hooks: {
        afterReplace: () => {
          if (++replacements === 3) throw new SimulatedPublicationCrash();
        },
      },
    }), SimulatedPublicationCrash);
    await recoverAgentCatalogPublication(root);
    assert.deepEqual(await officialState(root), before);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test('invalid recovery sources preserve the journal, backups and official files', async (t) => {
  for (const scenario of ['corrupt-backup', 'missing-backup', 'invalid-record']) {
    await t.test(scenario, async () => {
      const root = await initializePublicationRoot();
      try {
        let replacements = 0;
        await assert.rejects(() => generateAgentCatalog({
          repoRoot: root,
          ...generationFetchers(registryFixture('0.74.0')),
          hooks: {
            afterReplace: () => {
              if (++replacements === 3) throw new SimulatedPublicationCrash();
            },
          },
        }), SimulatedPublicationCrash);
        const journalPath = join(root, PUBLICATION_JOURNAL);
        const journal = JSON.parse(await readFile(journalPath, 'utf8'));
        if (scenario === 'corrupt-backup') {
          await writeFile(join(root, journal.records[1].backup), 'corrupt backup');
        } else if (scenario === 'missing-backup') {
          await rm(join(root, journal.records[1].backup));
        } else {
          journal.records.at(-1).operation = 'invalid';
          await writeFile(journalPath, JSON.stringify(journal));
        }
        const outputs = await officialState(root);
        const evidence = await publicationEvidence(root);
        await assert.rejects(() => recoverAgentCatalogPublication(root), /backup|recovery/i);
        assert.deepEqual(await officialState(root), outputs);
        assert.deepEqual(await publicationEvidence(root), evidence);
      } finally {
        await rm(root, { recursive: true, force: true });
      }
    });
  }
});

test('missing validation evidence aborts generation before downloads or publication', async (t) => {
  for (const scenario of ['missing-official-version', 'missing-local-version', 'invalid-local-version']) {
    await t.test(scenario, async () => {
      const root = await initializeGenerationRoot();
      try {
        const before = await officialState(root);
        const policy = officialPolicy();
        const registry = registryFixture('0.74.0');
        if (scenario === 'missing-official-version') {
          delete policy.validatedVersions.qoder;
        } else {
          const local = JSON.parse(await readFile(new URL('../resources/acp-registry-local.json', import.meta.url), 'utf8'));
          for (const agent of local.agents) {
            delete policy.validatedVersions[agent.id];
            registry.agents = registry.agents.filter((entry) => entry.id !== agent.id);
          }
          if (scenario === 'missing-local-version') {
            delete local.agents[0].provenance.validatedVersion;
          } else {
            local.agents[0].version = 'latest';
            local.agents[0].provenance.validatedVersion = 'latest';
          }
          await writeFile(join(root, 'resources', 'acp-registry-local.json'), JSON.stringify(local));
        }
        await writeFile(join(root, 'configs', 'agent-catalog-policy.json'), JSON.stringify(policy));
        let downloads = 0;
        await assert.rejects(() => generateAgentCatalog({
          repoRoot: root,
          ...generationFetchers(registry),
          fetchIcon: async () => { downloads += 1; return '<svg>unused</svg>'; },
        }), /validated version/i);
        assert.equal(downloads, 0);
        assert.deepEqual(await officialState(root), before);
        assert.equal(await recoverAgentCatalogPublication(root), false);
      } finally {
        await rm(root, { recursive: true, force: true });
      }
    });
  }
});
