import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const partsDir = path.join(__dirname, 'ko-parts');
const mapPath = path.join(__dirname, 'ko-path-map.json');

const enRoot = JSON.parse(fs.readFileSync(path.join(__dirname, '..', 'web', 'src', 'locales', 'en.json'), 'utf8'));
const map = JSON.parse(fs.readFileSync(mapPath, 'utf8'));

function applyMap(enNode, prefix) {
  if (typeof enNode === 'string') {
    if (!(prefix in map)) throw new Error(`Missing map entry: ${prefix}`);
    return map[prefix];
  }
  const out = {};
  for (const key of Object.keys(enNode)) {
    const next = prefix ? `${prefix}.${key}` : key;
    out[key] = applyMap(enNode[key], next);
  }
  return out;
}

for (const section of ['errors', 'sourceControl', 'settings', 'conversation']) {
  const translated = applyMap(enRoot[section], section);
  fs.writeFileSync(path.join(partsDir, `${section}.json`), `${JSON.stringify(translated, null, 2)}\n`, 'utf8');
  console.log('Wrote', section);
}
