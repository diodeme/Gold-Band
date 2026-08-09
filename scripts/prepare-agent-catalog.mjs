import { createHash } from 'node:crypto';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

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
];

const repoRoot = dirname(dirname(fileURLToPath(import.meta.url)));
const snapshotPath = join(repoRoot, 'resources', 'acp-registry.snapshot.json');
const catalogPath = join(repoRoot, 'resources', 'agent-catalog.json');
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
};

export function buildAgentCatalog(registry, fetchedAt = new Date().toISOString()) {
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
    return {
      id,
      label: override.label ?? agent.name,
      version: String(agent.version ?? ''),
      description: String(agent.description ?? ''),
      repository: agent.repository ?? null,
      website: agent.website ?? null,
      iconKey: override.iconKey ?? id,
      iconUrl: agent.icon,
      command: override.command ?? distribution.command,
      args: override.args ?? distribution.args,
      env: distribution.env,
      primaryAgentDir: override.primaryAgentDir ?? null,
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
  const registry = offline
    ? JSON.parse(await readFile(snapshotPath, 'utf8'))
    : await fetchJson(ACP_REGISTRY_URL);
  const raw = `${JSON.stringify(registry, null, 2)}\n`;
  const fetchedAt = process.env.SOURCE_DATE_EPOCH
    ? new Date(Number(process.env.SOURCE_DATE_EPOCH) * 1000).toISOString()
    : new Date().toISOString();
  const catalog = buildAgentCatalog(registry, fetchedAt);

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
