import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const root = path.join(__dirname, '..');
const localesDir = path.join(root, 'web', 'src', 'locales');
const partsDir = path.join(__dirname, 'ko-parts');

const en = JSON.parse(fs.readFileSync(path.join(localesDir, 'en.json'), 'utf8'));
const ko = {};

for (const key of Object.keys(en)) {
  const partPath = path.join(partsDir, `${key}.json`);
  if (!fs.existsSync(partPath)) {
    throw new Error(`Missing ko part: ${key}.json`);
  }
  ko[key] = JSON.parse(fs.readFileSync(partPath, 'utf8'));
}

function flatten(obj, prefix = '') {
  const result = {};
  for (const k of Object.keys(obj)) {
    const next = prefix ? `${prefix}.${k}` : k;
    if (typeof obj[k] === 'object' && obj[k] !== null && !Array.isArray(obj[k])) {
      Object.assign(result, flatten(obj[k], next));
    } else {
      result[next] = obj[k];
    }
  }
  return result;
}

const flatEn = flatten(en);
const flatKo = flatten(ko);
const enKeys = Object.keys(flatEn);
const koKeys = Object.keys(flatKo);
const keyMatch = JSON.stringify(enKeys) === JSON.stringify(koKeys);
const missing = enKeys.filter((k) => !(k in flatKo));
const extra = koKeys.filter((k) => !(k in flatEn));
const sameEnglish = enKeys.filter((k) => flatEn[k] === flatKo[k]).length;

const outPath = path.join(localesDir, 'ko-KR.json');
fs.writeFileSync(outPath, `${JSON.stringify(ko, null, 2)}\n`, 'utf8');

console.log('Wrote', outPath);
console.log('Key tree matches en.json:', keyMatch);
console.log('Missing keys:', missing.length, missing.slice(0, 5));
console.log('Extra keys:', extra.length, extra.slice(0, 5));
console.log('Strings identical to English:', sameEnglish);

if (!keyMatch || missing.length || extra.length) {
  process.exit(1);
}
