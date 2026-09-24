const fs = require('node:fs');
const path = require('node:path');

const partsDir = path.join(__dirname, 'ko-parts');
const en = require('../web/src/locales/en.json');

/** @type {Record<string, string>} */
const T = require('./en-ko-remaining-map.json');

function translateTree(node, prefix = '') {
  if (typeof node === 'string') {
    if (T[node]) return T[node];
    throw new Error(`Missing translation: ${prefix} -> ${node}`);
  }
  const out = {};
  for (const key of Object.keys(node)) {
    const next = prefix ? `${prefix}.${key}` : key;
    out[key] = translateTree(node[key], next);
  }
  return out;
}

for (const section of ['errors', 'sourceControl', 'settings', 'conversation']) {
  const ko = translateTree(en[section], section);
  fs.writeFileSync(path.join(partsDir, `${section}.json`), `${JSON.stringify(ko, null, 2)}\n`, 'utf8');
  console.log('Wrote', section);
}
