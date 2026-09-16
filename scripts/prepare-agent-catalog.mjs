import { createHash } from 'node:crypto';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import npa from 'npm-package-arg';

export const ACP_REGISTRY_URL = 'https://cdn.agentclientprotocol.com/registry/v1/latest/registry.json';
export const BUILTIN_AGENT_IDS = [
  'claude-acp',
  'codex-acp',
  'cursor',
  'gemini',
  'codebuddy-code',
  'goose',
  'qwen-code',
  'opencode',
  'kimi',
  'amp-acp',
  'pi-acp',
];

const repoRoot = dirname(dirname(fileURLToPath(import.meta.url)));
const snapshotPath = join(repoRoot, 'resources', 'acp-registry.snapshot.json');
const catalogPath = join(repoRoot, 'resources', 'agent-catalog.json');
const policyPath = join(repoRoot, 'configs', 'agent-catalog-policy.json');
const iconDir = join(repoRoot, 'web', 'public', 'agent-icons');

const overrides = {
  'claude-acp': {
    label: 'Claude', iconKey: 'claude', primaryAgentDir: '.claude', compatibleAgentDirs: [],
    supportsSystemPrompt: true,
  },
  'codex-acp': {
    label: 'Codex', iconKey: 'codex', primaryAgentDir: '.codex', compatibleAgentDirs: ['.agents'],
  },
  cursor: {
    label: 'Cursor', iconKey: 'cursor', command: 'cursor-agent', args: ['acp'],
    primaryAgentDir: '.cursor', compatibleAgentDirs: ['.agents'],
  },
  gemini: {
    label: 'Gemini', iconKey: 'gemini', primaryAgentDir: '.gemini', compatibleAgentDirs: ['.agents'],
  },
  'codebuddy-code': {
    label: 'CodeBuddy', iconKey: 'codebuddy-code', primaryAgentDir: '.codebuddy', compatibleAgentDirs: [],
  },
  goose: {
    label: 'Goose', iconKey: 'goose', command: 'goose', args: ['acp'],
    primaryAgentDir: '.goose', compatibleAgentDirs: [],
  },
  'qwen-code': {
    label: 'Qwen Code', iconKey: 'qwen-code', primaryAgentDir: '.qwen', compatibleAgentDirs: [],
  },
  opencode: {
    label: 'OpenCode', iconKey: 'opencode', command: 'opencode', args: ['acp'],
    primaryAgentDir: '.opencode', compatibleAgentDirs: ['.agents'],
  },
  kimi: {
    label: 'Kimi Code', iconKey: 'kimi', command: 'kimi', args: ['acp'],
    primaryAgentDir: '.kimi-code', compatibleAgentDirs: ['.agents'],
  },
  'amp-acp': {
    label: 'Amp', iconKey: 'amp-acp', command: 'amp-acp', args: [],
    primaryAgentDir: '.agents', compatibleAgentDirs: ['.claude'],
  },
  'pi-acp': {
    label: 'Pi', iconKey: 'pi-acp', primaryAgentDir: '.pi/agent',
    projectPrimaryAgentDir: '.pi', compatibleAgentDirs: ['.agents'],
  },
};

export function buildAgentCatalog(registry, fetchedAt = new Date().toISOString(), policy = {}) {
  if (!policy || typeof policy !== 'object' || Array.isArray(policy)
      || Object.keys(policy).some((key) => key !== 'versionPins')) {
    throw new Error('Invalid Agent catalog policy; expected versionPins.');
  }
  const pins = policy.versionPins === undefined ? {} : policy.versionPins;
  if (!pins || typeof pins !== 'object' || Array.isArray(pins)) {
    throw new Error('versionPins must be an object.');
  }
  for (const [id, version] of Object.entries(pins)) {
    if (!BUILTIN_AGENT_IDS.includes(id)) throw new Error(`Unknown Agent version pin: ${id}`);
    if (typeof version !== 'string' || !version || npa.resolve('version-pin', version).type !== 'version') {
      throw new Error(`Agent ${id} version pin must be an exact npm version.`);
    }
  }
  if (!registry || !Array.isArray(registry.agents)) {
    throw new Error('ACP Registry response does not contain an agents array.');
  }
  const byId = new Map(registry.agents.map((agent) => [agent.id, agent]));
  const missing = BUILTIN_AGENT_IDS.filter((id) => !byId.has(id));
  if (missing.length > 0) {
    throw new Error(`ACP Registry is missing required agents: ${missing.join(', ')}`);
  }

  const agents = BUILTIN_AGENT_IDS.map((id) => {
    const agent = byId.get(id);
    const override = overrides[id];
    const distribution = resolveDistributionDefaults(agent.distribution);
    const pin = pins[id];
    if (pin !== undefined) {
      if (!agent.distribution?.npx || override.command || override.args) {
        throw new Error(`Agent ${id} version pin requires an npx-managed template.`);
      }
      const spec = npa(agent.distribution.npx.package);
      if (!spec.registry || !spec.name || !['version', 'range', 'tag'].includes(spec.type)) {
        throw new Error(`Agent ${id} version pin requires an npm registry package.`);
      }
      distribution.args[1] = `${spec.name}@${pin}`;
    }
    return {
      id,
      label: override.label ?? agent.name,
      version: pin ?? String(agent.version ?? ''),
      description: String(agent.description ?? ''),
      repository: agent.repository ?? null,
      website: agent.website ?? null,
      iconKey: override.iconKey ?? id,
      iconUrl: agent.icon,
      command: override.command ?? distribution.command,
      args: override.args ?? distribution.args,
      env: distribution.env,
      primaryAgentDir: override.primaryAgentDir ?? null,
      projectPrimaryAgentDir: override.projectPrimaryAgentDir ?? null,
      compatibleAgentDirs: override.compatibleAgentDirs ?? [],
      supportsSystemPrompt: override.supportsSystemPrompt ?? false,
      supportsExternalSessionSync: false,
    };
  });

  return {
    schemaVersion: 1,
    source: {
      url: ACP_REGISTRY_URL,
      registryVersion: String(registry.version ?? ''),
      fetchedAt,
    },
    agents,
  };
}

function resolveDistributionDefaults(distribution = {}) {
  if (distribution.npx) {
    return {
      command: 'npx',
      args: ['-y', distribution.npx.package, ...(distribution.npx.args ?? [])],
      env: distribution.npx.env ?? {},
    };
  }
  if (distribution.uvx) {
    return {
      command: 'uvx',
      args: [distribution.uvx.package, ...(distribution.uvx.args ?? [])],
      env: distribution.uvx.env ?? {},
    };
  }
  return { command: '', args: [], env: {} };
}

async function fetchJson(url) {
  const response = await fetch(url, { headers: { accept: 'application/json' } });
  if (!response.ok) throw new Error(`Failed to fetch ${url}: HTTP ${response.status}`);
  return response.json();
}

async function downloadIcons(catalog) {
  await mkdir(iconDir, { recursive: true });
  for (const agent of catalog.agents) {
    if (!agent.iconUrl?.startsWith('https://')) {
      throw new Error(`Agent ${agent.id} does not provide an HTTPS icon URL.`);
    }
    const response = await fetch(agent.iconUrl, { headers: { accept: 'image/svg+xml' } });
    if (!response.ok) throw new Error(`Failed to fetch icon for ${agent.id}: HTTP ${response.status}`);
    const svg = await response.text();
    if (!svg.includes('<svg')) throw new Error(`Agent ${agent.id} icon is not SVG.`);
    await writeFile(join(iconDir, `${agent.iconKey}.svg`), svg, 'utf8');
  }
}

async function main() {
  const offline = process.argv.includes('--offline');
  const policy = JSON.parse(await readFile(policyPath, 'utf8'));
  const registry = offline
    ? JSON.parse(await readFile(snapshotPath, 'utf8'))
    : await fetchJson(ACP_REGISTRY_URL);
  const raw = `${JSON.stringify(registry, null, 2)}\n`;
  const fetchedAt = process.env.SOURCE_DATE_EPOCH
    ? new Date(Number(process.env.SOURCE_DATE_EPOCH) * 1000).toISOString()
    : new Date().toISOString();
  const catalog = buildAgentCatalog(registry, fetchedAt, policy);
  if (!offline) {
    for (const agent of catalog.agents) {
      if (!Object.hasOwn(policy.versionPins ?? {}, agent.id)) continue;
      const spec = npa(agent.args[1]);
      const url = `https://registry.npmjs.org/${encodeURIComponent(spec.name)}/${encodeURIComponent(spec.fetchSpec)}`;
      const response = await fetch(url, { signal: AbortSignal.timeout(30_000) });
      if (!response.ok) throw new Error(`Pinned package ${agent.args[1]} validation failed: HTTP ${response.status}`);
      await response.json();
    }
  }

  await mkdir(dirname(snapshotPath), { recursive: true });
  if (!offline) await writeFile(snapshotPath, raw, 'utf8');
  await writeFile(catalogPath, `${JSON.stringify(catalog, null, 2)}\n`, 'utf8');
  if (!offline) await downloadIcons(catalog);

  const digest = createHash('sha256').update(raw).digest('hex');
  console.log(`Prepared ${catalog.agents.length} Agent templates from ACP Registry ${catalog.source.registryVersion} (${digest.slice(0, 12)}).`);
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  main().catch((error) => {
    console.error(error instanceof Error ? error.message : String(error));
    process.exit(1);
  });
}
