import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const root = path.join(__dirname, '..');
const localesDir = path.join(root, 'web', 'src', 'locales');
const partsDir = path.join(__dirname, 'ko-parts');

const en = JSON.parse(fs.readFileSync(path.join(localesDir, 'en.json'), 'utf8'));
const zh = JSON.parse(fs.readFileSync(path.join(localesDir, 'zh-CN.json'), 'utf8'));

const KO = JSON.parse(fs.readFileSync(path.join(__dirname, 'ko-remaining-data.json'), 'utf8'));

function mergeSection(sectionKey) {
  const source = en[sectionKey];
  const translations = KO[sectionKey];
  if (!translations) throw new Error(`Missing section in ko-remaining-data.json: ${sectionKey}`);

  function walk(src, tr, pathParts = []) {
    if (typeof src === 'string') {
      const keyPath = [...pathParts, ''].join('.').slice(0, -1);
      const flatKey = sectionKey + (pathParts.length ? '.' + pathParts.join('.') : '');
      if (typeof tr === 'string') return tr;
      throw new Error(`Missing translation for ${flatKey}`);
    }
    const out = {};
    for (const key of Object.keys(src)) {
      out[key] = walk(src[key], tr[key], [...pathParts, key]);
    }
    return out;
  }

  return walk(source, translations, []);
}

for (const section of Object.keys(KO)) {
  const out = mergeSection(section);
  fs.writeFileSync(path.join(partsDir, `${section}.json`), `${JSON.stringify(out, null, 2)}\n`, 'utf8');
  console.log('Wrote', section);
}
