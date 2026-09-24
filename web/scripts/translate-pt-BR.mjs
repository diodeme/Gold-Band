import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const localesDir = path.join(__dirname, '../src/locales');
const cachePath = path.join(__dirname, '.pt-BR-cache.json');
const en = JSON.parse(fs.readFileSync(path.join(localesDir, 'en.json'), 'utf8'));
const zh = JSON.parse(fs.readFileSync(path.join(localesDir, 'zh-CN.json'), 'utf8'));

const STATIC_GLOSSARY = [
  'CI/CD',
  'Gold Band',
  'GitHub CLI',
  'GitHub',
  'Workflow',
  'worktree',
  'Worktree',
  'Profile',
  'runtime',
  'AUTO',
  'Direct',
  'Agent',
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
];

const PLACEHOLDER_RE = /\{\{[^}]+\}\}/g;
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

function protectString(text, glossaryTokens) {
  const placeholders = [];
  let protectedText = text.replace(PLACEHOLDER_RE, (match) => {
    const id = placeholders.length;
    placeholders.push(match);
    return `__PH${id}__`;
  });

  const glossary = [];
  for (const token of glossaryTokens) {
    if (!protectedText.includes(token)) continue;
    const re = new RegExp(token.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'), 'g');
    protectedText = protectedText.replace(re, () => {
      const id = glossary.length;
      glossary.push(token);
      return `__GL${id}__`;
    });
  }

  return { protectedText, placeholders, glossary };
}

function restoreString(text, placeholders, glossary) {
  let restored = text;
  for (let i = 0; i < glossary.length; i += 1) {
    restored = restored.split(`__GL${i}__`).join(glossary[i]);
    restored = restored.split(`__GL ${i}__`).join(glossary[i]);
    restored = restored.split(`__ GL${i}__`).join(glossary[i]);
  }
  for (let i = 0; i < placeholders.length; i += 1) {
    restored = restored.split(`__PH${i}__`).join(placeholders[i]);
    restored = restored.split(`__PH ${i}__`).join(placeholders[i]);
    restored = restored.split(`__ PH${i}__`).join(placeholders[i]);
  }
  return restored;
}

async function translateText(text) {
  const url = `https://api.mymemory.translated.net/get?q=${encodeURIComponent(text)}&langpair=en|pt-BR`;
  for (let attempt = 0; attempt < 8; attempt += 1) {
    try {
      const response = await fetch(url);
      const data = await response.json();
      if (data.responseStatus === 200 && data.responseData?.translatedText) {
        return data.responseData.translatedText;
      }
      if (data.quotaFinished || data.responseStatus === 429) {
        await sleep(2000 * (attempt + 1));
        continue;
      }
    } catch {
      await sleep(2000 * (attempt + 1));
    }
  }
  return null;
}

function walkPairs(enObj, zhObj, pathParts = []) {
  const pairs = [];
  for (const key of Object.keys(enObj)) {
    const nextPath = [...pathParts, key];
    if (typeof enObj[key] === 'object' && enObj[key] !== null && !Array.isArray(enObj[key])) {
      pairs.push(...walkPairs(enObj[key], zhObj[key], nextPath));
    } else {
      pairs.push({
        path: nextPath,
        en: enObj[key],
        zh: zhObj[key],
      });
    }
  }
  return pairs;
}

function setByPath(root, pathParts, value) {
  let current = root;
  for (let i = 0; i < pathParts.length - 1; i += 1) {
    current = current[pathParts[i]];
  }
  current[pathParts[pathParts.length - 1]] = value;
}

function cloneStructure(obj) {
  if (typeof obj !== 'object' || obj === null || Array.isArray(obj)) return obj;
  const out = {};
  for (const key of Object.keys(obj)) {
    out[key] = cloneStructure(obj[key]);
  }
  return out;
}

const diskCache = fs.existsSync(cachePath)
  ? JSON.parse(fs.readFileSync(cachePath, 'utf8'))
  : {};

function saveCache() {
  fs.writeFileSync(cachePath, JSON.stringify(diskCache, null, 2), 'utf8');
}

const pairs = walkPairs(en, zh);
const uniqueEntries = [];
const seen = new Set();
for (const pair of pairs) {
  if (seen.has(pair.en)) continue;
  seen.add(pair.en);
  uniqueEntries.push({
    en: pair.en,
    zh: pair.zh,
    glossaryTokens: collectGlossaryTokens(pair.en, pair.zh),
  });
}

const pending = uniqueEntries.filter((entry) => !diskCache[entry.en]);
console.log(`Total unique: ${uniqueEntries.length}, cached: ${uniqueEntries.length - pending.length}, pending: ${pending.length}`);

let done = 0;
for (const entry of pending) {
  const { protectedText, placeholders, glossary } = protectString(entry.en, entry.glossaryTokens);
  let translated = protectedText;
  if (protectedText.trim()) {
    translated = await translateText(protectedText);
    if (translated === null) {
      console.error(`Failed to translate, stopping at ${done}/${pending.length}: ${entry.en.slice(0, 100)}`);
      saveCache();
      process.exit(1);
    }
    await sleep(350);
  }
  diskCache[entry.en] = restoreString(translated, placeholders, glossary);
  done += 1;
  if (done % 25 === 0 || done === pending.length) {
    saveCache();
    console.log(`Progress: ${done}/${pending.length}`);
  }
}

const ptBR = cloneStructure(en);
for (const pair of pairs) {
  setByPath(ptBR, pair.path, diskCache[pair.en] ?? pair.en);
}

const outPath = path.join(localesDir, 'pt-BR.json');
fs.writeFileSync(outPath, `${JSON.stringify(ptBR, null, 2)}\n`, 'utf8');
console.log(`Wrote ${outPath}`);
