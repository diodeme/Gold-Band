import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const localesDir = path.join(__dirname, '../src/locales');
const mapDir = path.join(__dirname, 'pt-translations');

const en = JSON.parse(fs.readFileSync(path.join(localesDir, 'en.json'), 'utf8'));

const translationMap = {};
for (const file of fs.readdirSync(mapDir).filter((f) => f.endsWith('.json')).sort()) {
  Object.assign(translationMap, JSON.parse(fs.readFileSync(path.join(mapDir, file), 'utf8')));
}

function translateLeaf(value) {
  if (typeof value !== 'string') return value;
  if (Object.prototype.hasOwnProperty.call(translationMap, value)) {
    return translationMap[value];
  }
  throw new Error(`Missing translation: ${value}`);
}

function walk(obj) {
  if (typeof obj !== 'object' || obj === null || Array.isArray(obj)) {
    return translateLeaf(obj);
  }
  const out = {};
  for (const key of Object.keys(obj)) {
    out[key] = walk(obj[key]);
  }
  return out;
}

const missing = [];
function collectMissing(obj) {
  if (typeof obj !== 'object' || obj === null || Array.isArray(obj)) {
    if (typeof obj === 'string' && !Object.prototype.hasOwnProperty.call(translationMap, obj)) {
      missing.push(obj);
    }
    return;
  }
  for (const key of Object.keys(obj)) collectMissing(obj[key]);
}

collectMissing(en);
if (missing.length > 0) {
  const uniq = [...new Set(missing)];
  console.error(`Missing ${uniq.length} translations`);
  fs.writeFileSync(path.join(mapDir, '_missing.json'), JSON.stringify(uniq, null, 2), 'utf8');
  process.exit(1);
}

const ptBR = walk(en);
const outPath = path.join(localesDir, 'pt-BR.json');
fs.writeFileSync(outPath, `${JSON.stringify(ptBR, null, 2)}\n`, 'utf8');
console.log(`Wrote ${outPath}`);
