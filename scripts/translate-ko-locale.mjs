import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { translate } from '@vitalets/google-translate-api';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const root = path.join(__dirname, '..');
const localesDir = path.join(root, 'web', 'src', 'locales');
const cachePath = path.join(root, 'scripts', '.ko-translate-cache.json');

const GLOSSARY_TOKENS = [
  'CI/CD',
  'Gold Band',
  'AI-DYNAMIC',
  'ACP Registry',
  'API Key',
  'GitHub API',
  'GitHub CLI',
  'GitHub',
  'Workflow',
  'worktree',
  'worktrees',
  'Profile',
  'Profiles',
  'runtime',
  'AUTO',
  'Direct',
  'Agent',
  'Agents',
  'Git',
  'MCP',
  'ACP',
  'Cron',
  'JSON',
  'SKILL',
  'SKILLs',
  'Multica',
  'MiniJinja',
  'Markdown',
  'WebP',
  'PNG',
  'JPEG',
  'SVG',
  'HTML',
  'HTTP',
  'SSE',
  'Stdio',
  'UTF-8',
  'Fetch',
  'Pull',
  'Push',
  'HEAD',
  'KEY=VALUE',
  'Ctrl+V',
  'Codex',
  'Claude',
  'Cursor',
  'Gemini',
  'OpenCode',
  'progress.events',
  'raw.stream',
];

const PLACEHOLDER_RE = /\{\{[^}]+\}\}/g;
const TAG_RE = /<\/?[^>]+>/g;
const DELAY_MS = 900;
const MAX_RETRIES = 6;

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function sortKeysLike(source, target) {
  if (typeof source !== 'object' || source === null || Array.isArray(source)) {
    return target;
  }
  const out = {};
  for (const key of Object.keys(source)) {
    out[key] = sortKeysLike(source[key], target?.[key]);
  }
  return out;
}

function flatten(obj, prefix = '') {
  const result = {};
  for (const key of Object.keys(obj)) {
    const next = prefix ? `${prefix}.${key}` : key;
    if (typeof obj[key] === 'object' && obj[key] !== null && !Array.isArray(obj[key])) {
      Object.assign(result, flatten(obj[key], next));
    } else {
      result[next] = obj[key];
    }
  }
  return result;
}

function unflatten(flat) {
  const out = {};
  for (const [pathKey, value] of Object.entries(flat)) {
    const parts = pathKey.split('.');
    let cur = out;
    for (let i = 0; i < parts.length - 1; i += 1) {
      cur[parts[i]] ??= {};
      cur = cur[parts[i]];
    }
    cur[parts[parts.length - 1]] = value;
  }
  return out;
}

function tokensForKey(zhValue) {
  if (typeof zhValue !== 'string') return [];
  const found = [];
  for (const token of GLOSSARY_TOKENS) {
    if (zhValue.includes(token)) found.push(token);
  }
  return found.sort((a, b) => b.length - a.length);
}

function protectString(text, extraTokens = []) {
  const placeholders = [];
  const tags = [];
  const tokens = [];

  let protectedText = text.replace(PLACEHOLDER_RE, (match) => {
    const id = `__PH_${placeholders.length}__`;
    placeholders.push(match);
    return id;
  });

  protectedText = protectedText.replace(TAG_RE, (match) => {
    const id = `__TAG_${tags.length}__`;
    tags.push(match);
    return id;
  });

  const allTokens = [...new Set([...extraTokens, ...GLOSSARY_TOKENS])].sort((a, b) => b.length - a.length);
  for (const token of allTokens) {
    if (!protectedText.includes(token)) continue;
    const id = `__TOK_${tokens.length}__`;
    tokens.push(token);
    protectedText = protectedText.split(token).join(id);
  }

  return { protectedText, placeholders, tags, tokens };
}

function restoreString(text, { placeholders, tags, tokens }) {
  let restored = text;
  for (let i = 0; i < tokens.length; i += 1) {
    restored = restored.split(`__TOK_${i}__`).join(tokens[i]);
  }
  for (let i = 0; i < tags.length; i += 1) {
    restored = restored.split(`__TAG_${i}__`).join(tags[i]);
  }
  for (let i = 0; i < placeholders.length; i += 1) {
    restored = restored.split(`__PH_${i}__`).join(placeholders[i]);
  }
  return restored;
}

function loadCache() {
  if (!fs.existsSync(cachePath)) return {};
  try {
    return JSON.parse(fs.readFileSync(cachePath, 'utf8'));
  } catch {
    return {};
  }
}

function saveCache(cache) {
  fs.writeFileSync(cachePath, JSON.stringify(cache, null, 2), 'utf8');
}

async function translateWithRetry(text, extraTokens) {
  const pack = protectString(text, extraTokens);
  if (!pack.protectedText.trim()) return text;

  let lastError;
  for (let attempt = 0; attempt < MAX_RETRIES; attempt += 1) {
    try {
      await sleep(DELAY_MS + attempt * 500);
      const { text: translated } = await translate(pack.protectedText, { from: 'en', to: 'ko' });
      return restoreString(translated, pack);
    } catch (error) {
      lastError = error;
      const wait = 5000 * (attempt + 1);
      console.warn(`Retry ${attempt + 1}/${MAX_RETRIES} after error: ${error.message}. Waiting ${wait}ms`);
      await sleep(wait);
    }
  }
  throw lastError;
}

async function main() {
  const en = JSON.parse(fs.readFileSync(path.join(localesDir, 'en.json'), 'utf8'));
  const zh = JSON.parse(fs.readFileSync(path.join(localesDir, 'zh-CN.json'), 'utf8'));
  const flatEn = flatten(en);
  const flatZh = flatten(zh);
  const cache = loadCache();
  const flatKo = {};

  const keys = Object.keys(flatEn);
  for (let i = 0; i < keys.length; i += 1) {
    const key = keys[i];
    const value = flatEn[key];
    if (cache[value]) {
      flatKo[key] = cache[value];
      continue;
    }
    const extraTokens = tokensForKey(flatZh[key]);
    const translated = await translateWithRetry(value, extraTokens);
    cache[value] = translated;
    flatKo[key] = translated;
    if ((i + 1) % 25 === 0) {
      saveCache(cache);
      console.log(`Progress ${i + 1}/${keys.length}`);
    }
  }

  saveCache(cache);
  const ko = sortKeysLike(en, unflatten(flatKo));
  const outPath = path.join(localesDir, 'ko-KR.json');
  fs.writeFileSync(outPath, `${JSON.stringify(ko, null, 2)}\n`, 'utf8');

  const written = JSON.parse(fs.readFileSync(outPath, 'utf8'));
  const flatWritten = flatten(written);
  const keyMatch = JSON.stringify(Object.keys(flatEn)) === JSON.stringify(Object.keys(flatWritten));
  const sameEnglish = Object.keys(flatEn).filter((k) => flatEn[k] === flatWritten[k]).length;

  console.log('Done.');
  console.log('Cache entries:', Object.keys(cache).length);
  console.log('Key tree matches en.json:', keyMatch);
  console.log('Strings still identical to English:', sameEnglish);
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
