import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const root = path.join(__dirname, '..');
const partsDir = path.join(__dirname, 'ko-parts');
const cachePath = path.join(__dirname, 'en-ko-remaining-cache.json');

const GLOSSARY = [
  'CI/CD', 'Gold Band', 'AI-DYNAMIC', 'ACP Registry', 'API Key', 'GitHub API', 'GitHub CLI', 'GitHub',
  'Workflow', 'worktree', 'worktrees', 'Profile', 'Profiles', 'runtime', 'AUTO', 'Direct', 'Agent', 'Agents',
  'Git', 'MCP', 'ACP', 'Cron', 'JSON', 'SKILL', 'Multica', 'Fetch', 'Pull', 'Push', 'HEAD', 'Markdown',
  'WebP', 'PNG', 'JPEG', 'SVG', 'HTML', 'HTTP', 'SSE', 'Stdio', 'UTF-8', 'Ctrl+V', 'MiniJinja', 'Claude',
  'Codex', 'Cursor', 'Gemini', 'OpenCode', 'progress.events', 'raw.stream', '.gold-band', '.claude', '.pi',
  '.agents', 'KEY=VALUE', 'Provider', 'Session', 'Desktop', 'Porcelain', 'Tech Gray', 'Graphite Dark',
  'Terminal Black', 'WeCom', 'DEBUG', 'INFO', 'Sub-agent', 'Context Window', 'Token Usage', 'Cache Read',
  'Cache Write', 'Initialize', 'Inbound', 'Outbound', 'Auto Accept', 'System prompt', 'Raw frames',
  'Tool call', 'Tool output', 'Tool parameters', 'Bootstrap', 'Fanout', 'Detached HEAD', 'Runtime checkpoint',
  'Pull Requests', 'End node', 'New Round', 'Canvas', 'Inspector', 'doctor', 'workflow.id', '$new-round',
  '$entry', 'merge', 'acceptance', 'AI Dynamic Node', 'Dynamic Worker Node', 'Dynamic Workflow Node',
  'Dynamic Merge Node', 'Dynamic Acceptance Node', 'Unknown Node', 'Agent node', 'ACP session', 'ACP Session',
  'ACP turn finished', 'outer run', 'outer-run', 'Gold Band Logo', 'Gold Band Shared Memory', 'Gold Band Git',
  'Gold Band Workflow Control', 'runtime.log', 'runtime-internal', 'AI output decision', 'AI-DYNAMIC routing',
  'AI Output Validation', 'JSON Schema', 'JSON output constraint', 'Beautify JSON', 'Invalid JSON',
  'Success expression', 'Permission mode', 'Bootstrap agent', 'Dynamic agent', 'Fixed agent',
  'Available dynamic agents', 'Agent routing guidance', 'Max Dynamic Nodes', 'Max Fanout', 'Max Depth',
  'Max Parallel', 'Group Depth', 'Max Workflow Invocations', 'Allow nested AI-DYNAMIC', 'Manage Profiles',
  'Node Config', 'Edge Config', 'Workflow Controls', 'Workflow Settings', 'Default Template', 'Save Workflow',
  'Workflow Editor', 'Workflow ID', 'Entry node', 'New Round Start', 'Edge Type', 'Manual check',
  'Output artifact key', 'Allowed Workflows', 'Available profiles', 'Global goal', 'Selectable workflows',
  'Unavailable workflows', 'Sync model configuration', 'Model Configuration', 'Unspecified', 'Quick add successor',
  'Current Selection', 'Review and fix', 'Fast-forward only (recommended)', 'Pull request', 'pull request',
  'Source Control', 'Git file diff', 'Git operation failed', 'Git workflow', 'Git version', 'Git repository',
  'Git features', 'Git workspace', 'Git downloads', 'Git reason', 'Git write', 'Create worktree', 'Remove worktree',
  'Commit review', 'Commit reachability', 'Commit list', 'Commit changes', 'Commit subject', 'Commit body',
  'Stash', 'Rebase', 'Merge', 'Push tag', 'Pull strategy', 'Working tree is clean', 'Initialize repository',
  'Runtime-internal branches', 'AUTO mode', 'Direct mode', 'AUTO Template', 'AUTO outer run', 'Workflow run',
  'Workflow template', 'Workflow mode', 'Workflow Control', 'Workflow Blueprint', 'Edit AUTO', 'Edit Workflow',
  'Configure AUTO', 'Configure Run Mode', 'Run Mode', 'Run Mode Management', 'Requirement Interview',
  'Requirement Grilling', 'Metrics Reporting', 'Verbose runtime logging', 'Use local Claude',
  'IM remote intervention and notifications', 'Native ACP permission mode', 'Direct Agent',
  'Direct reply completion rate', 'Direct mode', 'Profile catalog', 'runtime controlled', 'runtime context',
  'runtime abnormal', 'runtime error', 'runtime session', 'Cron expression', 'six-field Cron expression',
  'Update the JSON', 'legacy JSON Schema', 'simplified output shape', 'Multica ·', 'Daemon', 'Issue',
  'Issues', 'OpenCode', 'Beta', 'WB', 'Todo', 'Done', 'IM', 'QR', 'SHA', 'Codex', 'Claude', 'Cursor',
  'Gemini', 'OpenCode', 'CodeMirror', 'Front matter', 'front matter', 'Provider', 'Multica Web', 'Multica',
  'worktree support', 'worktree state', 'worktree path', 'Worktrees', 'worktree', 'Head branch', 'Base branch',
  'Pull Request', 'Pull request', 'Pull Requests', 'GitHub CLI', 'GitHub repository', 'GitHub browser login',
  'GitHub operation', 'GitHub API', 'GitHub token', 'GitHub', 'Attempt', 'attempt', 'Round', 'round',
  'new round', 'New Round', 'new-round', 'End node', 'end node', 'Entry', 'entry', 'Fanout', 'fanout',
  'Bootstrap', 'bootstrap', 'Sub-agent', 'sub-agent', 'Sub-agent prompt', 'Sub-agent result',
];

const PH_RE = /\{\{[^}]+\}\}/g;
const TAG_RE = /<\/?[^>]+>/g;
const DELAY_MS = 350;

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

function protect(text, zhText = '') {
  const placeholders = [];
  const tags = [];
  const tokens = [];
  let out = text.replace(PH_RE, (m) => {
    placeholders.push(m);
    return `__PH${placeholders.length - 1}__`;
  });
  out = out.replace(TAG_RE, (m) => {
    tags.push(m);
    return `__TG${tags.length - 1}__`;
  });
  const extra = GLOSSARY.filter((t) => (zhText && zhText.includes(t)) || out.includes(t));
  for (const token of [...new Set([...extra, ...GLOSSARY])].sort((a, b) => b.length - a.length)) {
    if (!out.includes(token)) continue;
    tokens.push(token);
    out = out.split(token).join(`__TK${tokens.length - 1}__`);
  }
  return { out, placeholders, tags, tokens };
}

function restore(text, pack) {
  let out = text;
  for (let i = 0; i < pack.tokens.length; i += 1) out = out.split(`__TK${i}__`).join(pack.tokens[i]);
  for (let i = 0; i < pack.tags.length; i += 1) out = out.split(`__TG${i}__`).join(pack.tags[i]);
  for (let i = 0; i < pack.placeholders.length; i += 1) out = out.split(`__PH${i}__`).join(pack.placeholders[i]);
  return out;
}

async function translateGoogle(text, zhText) {
  const pack = protect(text, zhText);
  if (!pack.out.trim()) return text;
  const url = `https://clients5.google.com/translate_a/t?client=dict-chrome-experiment-beta&sl=en&tl=ko&q=${encodeURIComponent(pack.out)}`;
  const res = await fetch(url);
  if (!res.ok) throw new Error(`HTTP ${res.status}`);
  const data = await res.json();
  const translated = Array.isArray(data?.[0]) ? data[0][0] : data?.[0] ?? text;
  return restore(String(translated), pack);
}

function flatSection(obj, prefix) {
  const out = {};
  for (const key of Object.keys(obj)) {
    const next = `${prefix}.${key}`;
    if (typeof obj[key] === 'string') out[next] = obj[key];
    else Object.assign(out, flatSection(obj[key], next));
  }
  return out;
}

function unflatten(flat, prefix) {
  const out = {};
  for (const [pathKey, value] of Object.entries(flat)) {
    if (!pathKey.startsWith(`${prefix}.`)) continue;
    const parts = pathKey.slice(prefix.length + 1).split('.');
    let cur = out;
    for (let i = 0; i < parts.length - 1; i += 1) {
      cur[parts[i]] ??= {};
      cur = cur[parts[i]];
    }
    cur[parts[parts.length - 1]] = value;
  }
  return out;
}

const en = JSON.parse(fs.readFileSync(path.join(root, 'web', 'src', 'locales', 'en.json'), 'utf8'));
const zh = JSON.parse(fs.readFileSync(path.join(root, 'web', 'src', 'locales', 'zh-CN.json'), 'utf8'));
const reuse = JSON.parse(fs.readFileSync(path.join(__dirname, 'en-ko-reuse-map.json'), 'utf8'));
const cache = fs.existsSync(cachePath) ? JSON.parse(fs.readFileSync(cachePath, 'utf8')) : { ...reuse };

const sections = ['errors', 'sourceControl', 'settings', 'conversation'];
const enFlat = {};
const zhFlat = {};
for (const section of sections) {
  Object.assign(enFlat, flatSection(en[section], section));
  Object.assign(zhFlat, flatSection(zh[section], section));
}

let i = 0;
for (const [pathKey, enText] of Object.entries(enFlat)) {
  i += 1;
  if (cache[enText]) continue;
  await sleep(DELAY_MS);
  cache[enText] = await translateGoogle(enText, zhFlat[pathKey]);
  if (i % 20 === 0) {
    fs.writeFileSync(cachePath, JSON.stringify(cache, null, 2));
    console.log(`Progress ${i}/${Object.keys(enFlat).length}`);
  }
}
fs.writeFileSync(cachePath, JSON.stringify(cache, null, 2));
fs.writeFileSync(path.join(__dirname, 'en-ko-remaining-map.json'), JSON.stringify(cache, null, 2));

for (const section of sections) {
  const translatedFlat = {};
  for (const [pathKey, enText] of Object.entries(enFlat)) {
    if (!pathKey.startsWith(`${section}.`)) continue;
    translatedFlat[pathKey] = cache[enText] ?? enText;
  }
  const koSection = unflatten(translatedFlat, section);
  fs.writeFileSync(path.join(partsDir, `${section}.json`), `${JSON.stringify(koSection, null, 2)}\n`, 'utf8');
  console.log('Wrote section', section);
}

console.log('Done remaining sections');
