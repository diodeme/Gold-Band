import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const localesDir = path.join(__dirname, '../src/locales');
const en = JSON.parse(fs.readFileSync(path.join(localesDir, 'en.json'), 'utf8'));
const cachePath = path.join(localesDir, '.es-translation-cache.json');
const overridesPath = path.join(localesDir, '.es-overrides.json');

const cache = fs.existsSync(cachePath) ? JSON.parse(fs.readFileSync(cachePath, 'utf8')) : {};
const overrides = fs.existsSync(overridesPath) ? JSON.parse(fs.readFileSync(overridesPath, 'utf8')) : {};

function walk(enObj, pathParts = []) {
  for (const key of Object.keys(enObj)) {
    const nextPath = [...pathParts, key];
    if (typeof enObj[key] === 'object' && enObj[key] !== null && !Array.isArray(enObj[key])) {
      walk(enObj[key], nextPath);
    } else if (typeof enObj[key] === 'string') {
      const value = enObj[key];
      if (!overrides[value] && !cache[value]) {
        throw new Error(`Missing translation for ${nextPath.join('.')}: ${value}`);
      }
      enObj[key] = overrides[value] ?? cache[value];
    }
  }
}

const es = JSON.parse(fs.readFileSync(path.join(localesDir, 'en.json'), 'utf8'));
walk(es);

const outPath = path.join(localesDir, 'es.json');
fs.writeFileSync(outPath, `${JSON.stringify(es, null, 2)}\n`, 'utf8');
console.log(`Wrote ${outPath}`);
