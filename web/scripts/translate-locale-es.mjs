import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const localesDir = path.join(__dirname, '../src/locales');
const en = JSON.parse(fs.readFileSync(path.join(localesDir, 'en.json'), 'utf8'));
const zh = JSON.parse(fs.readFileSync(path.join(localesDir, 'zh-CN.json'), 'utf8'));

const STATIC_GLOSSARY = [
  'CI/CD',
  'Gold Band Shared Memory',
  'Gold Band Workflow Control',
  'Gold Band',
  'AI-DYNAMIC',
  'ACP Registry',
  'GitHub CLI',
  'GitHub',
  'Workflow',
  'worktree',
  'worktrees',
  'Worktree',
  'Worktrees',
  'Profile',
  'Profiles',
  'runtime',
  'Runtime',
  'AUTO',
  'Direct',
  'Agent',
  'Agents',
  'Cron',
  'JSON',
  'MCP',
  'ACP',
  'API',
  'Git',
  'Merge',
  'Rebase',
  'Commit',
  'Fetch',
  'Pull',
  'Push',
  'Stash',
  'doctor',
  'SKILL',
  'SKILLs',
  'Markdown',
  'MiniJinja',
  'CodeMirror',
  'WeCom',
  'Multica',
  'Provider',
  'HEAD',
  'SSE',
  'HTTP',
  'Stdio',
  'PNG',
  'JPEG',
  'WebP',
  'SVG',
  'URL',
  'SDK',
  'Beta',
  'localhost',
  'Porcelain',
  'Graphite Dark',
  'Tech Gray',
  'Terminal Black',
  'KEY=VALUE',
  'Claude',
  'Codex',
  'Cursor',
  'Gemini',
  'OpenCode',
];

const PLACEHOLDER_RE = /(\{\{[^}]+\}\}|<\/?[^>]+>|\$[a-zA-Z-]+)/g;
const LATIN_TOKEN_RE = /[A-Za-z][A-Za-z0-9_\-/]*/g;

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function collectGlossaryTokens(enText, zhText) {
  const tokens = new Set(STATIC_GLOSSARY);
  if (typeof enText !== 'string' || typeof zhText !== 'string') return [...tokens];
  for (const match of enText.matchAll(LATIN_TOKEN_RE)) {
    const token = match[0];
    if (zhText.includes(token)) tokens.add(token);
  }
  for (const token of STATIC_GLOSSARY) {
    if (enText.includes(token) && zhText.includes(token)) tokens.add(token);
  }
  return [...tokens].sort((a, b) => b.length - a.length);
}

function applyGlossary(text, glossaryTokens) {
  let out = text;
  for (const token of glossaryTokens) {
    const re = new RegExp(token.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'), 'gi');
    out = out.replace(re, token);
  }
  return out;
}

async function translateSegment(text) {
  if (!text.trim()) return text;
  const url = `https://translate.googleapis.com/translate_a/single?client=gtx&sl=en&tl=es&dt=t&q=${encodeURIComponent(text)}`;
  for (let attempt = 0; attempt < 10; attempt += 1) {
    const response = await fetch(url);
    if (response.status === 429) {
      await sleep(3000 * (attempt + 1));
      continue;
    }
    if (!response.ok) {
      await sleep(1500 * (attempt + 1));
      continue;
    }
    const data = await response.json();
    return data[0].map((part) => part[0]).join('');
  }
  throw new Error(`Segment translation failed: ${text.slice(0, 80)}`);
}

async function translateString(text, glossaryTokens) {
  const parts = text.split(PLACEHOLDER_RE);
  const translatedParts = [];
  for (const part of parts) {
    if (!part) continue;
    if (part.startsWith('{{') || part.startsWith('<') || part.startsWith('$')) {
      translatedParts.push(part);
    } else {
      translatedParts.push(await translateSegment(part));
      await sleep(1200);
    }
  }
  return applyGlossary(translatedParts.join(''), glossaryTokens);
}

function walkPairs(enObj, zhObj, pathParts = []) {
  const pairs = [];
  for (const key of Object.keys(enObj)) {
    const nextPath = [...pathParts, key];
    const zhVal = zhObj?.[key];
    if (typeof enObj[key] === 'object' && enObj[key] !== null && !Array.isArray(enObj[key])) {
      pairs.push(...walkPairs(enObj[key], zhVal && typeof zhVal === 'object' ? zhVal : {}, nextPath));
    } else {
      pairs.push({ path: nextPath, en: enObj[key], zh: zhVal });
    }
  }
  return pairs;
}

function setByPath(root, pathParts, value) {
  let current = root;
  for (let i = 0; i < pathParts.length - 1; i += 1) current = current[pathParts[i]];
  current[pathParts[pathParts.length - 1]] = value;
}

function cloneStructure(obj) {
  if (typeof obj !== 'object' || obj === null || Array.isArray(obj)) return obj;
  const out = {};
  for (const key of Object.keys(obj)) out[key] = cloneStructure(obj[key]);
  return out;
}

function keyTreeEqual(a, b, path = '') {
  const ak = Object.keys(a);
  const bk = Object.keys(b);
  if (ak.length !== bk.length) {
    console.error(`Key count mismatch at ${path || '<root>'}`);
    return false;
  }
  let ok = true;
  for (const key of ak) {
    if (!Object.prototype.hasOwnProperty.call(b, key)) ok = false;
    else if (typeof a[key] === 'object' && a[key]) ok = keyTreeEqual(a[key], b[key], `${path}.${key}`) && ok;
    else if (typeof a[key] !== typeof b[key]) ok = false;
  }
  return ok;
}

function placeholderEqual(a, b, path = '') {
  let ok = true;
  for (const key of Object.keys(a)) {
    const av = a[key];
    const bv = b[key];
    const p = path ? `${path}.${key}` : key;
    if (typeof av === 'string') {
      const ap = [...av.matchAll(/\{\{[^}]+\}\}/g)].map((m) => m[0]).sort().join('|');
      const bp = [...bv.matchAll(/\{\{[^}]+\}\}/g)].map((m) => m[0]).sort().join('|');
      if (ap !== bp) {
        console.error(`Placeholder mismatch at ${p}`);
        ok = false;
      }
    } else if (av && typeof av === 'object') ok = placeholderEqual(av, bv, p) && ok;
  }
  return ok;
}

const cachePath = path.join(localesDir, '.es-translation-cache.json');
const diskCache = fs.existsSync(cachePath) ? JSON.parse(fs.readFileSync(cachePath, 'utf8')) : {};
const pairs = walkPairs(en, zh);
const uniqueEntries = [];
const meta = new Map();

for (const pair of pairs) {
  if (!meta.has(pair.en)) {
    meta.set(pair.en, collectGlossaryTokens(pair.en, pair.zh));
    if (!diskCache[pair.en]) uniqueEntries.push(pair.en);
  }
}

console.log(`Translating ${uniqueEntries.length} remaining unique strings (${pairs.length} total)...`);

let done = 0;
for (const enText of uniqueEntries) {
  try {
    diskCache[enText] = await translateString(enText, meta.get(enText));
  } catch (error) {
    console.error(`Failed: ${enText.slice(0, 80)} -> ${error.message}`);
    throw error;
  }
  done += 1;
  if (done % 10 === 0) {
    fs.writeFileSync(cachePath, JSON.stringify(diskCache), 'utf8');
    console.log(`Progress: ${done}/${uniqueEntries.length}`);
  }
}

fs.writeFileSync(cachePath, JSON.stringify(diskCache), 'utf8');

const es = cloneStructure(en);
for (const pair of pairs) setByPath(es, pair.path, diskCache[pair.en]);

const outPath = path.join(localesDir, 'es.json');
fs.writeFileSync(outPath, `${JSON.stringify(es, null, 2)}\n`, 'utf8');

console.log(JSON.stringify({
  treeMatch: keyTreeEqual(en, es),
  placeholderMatch: placeholderEqual(en, es),
  outPath,
}, null, 2));
