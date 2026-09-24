import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const localesDir = path.join(__dirname, '../src/locales');
const en = JSON.parse(fs.readFileSync(path.join(localesDir, 'en.json'), 'utf8'));
const pt = JSON.parse(fs.readFileSync(path.join(localesDir, 'pt-BR.json'), 'utf8'));

function keyPaths(obj, prefix = '') {
  const paths = [];
  for (const key of Object.keys(obj)) {
    const next = prefix ? `${prefix}.${key}` : key;
    if (typeof obj[key] === 'object' && obj[key] !== null && !Array.isArray(obj[key])) {
      paths.push(...keyPaths(obj[key], next));
    } else {
      paths.push(next);
    }
  }
  return paths;
}

function get(obj, dotPath) {
  return dotPath.split('.').reduce((current, key) => current[key], obj);
}

const enPaths = keyPaths(en);
const ptPaths = keyPaths(pt);
console.log('en leaf keys:', enPaths.length);
console.log('pt leaf keys:', ptPaths.length);
console.log('paths match:', enPaths.join('|') === ptPaths.join('|'));
console.log('top-level order match:', JSON.stringify(Object.keys(en)) === JSON.stringify(Object.keys(pt)));

const sameAsEn = [];
function walk(enObj, ptObj, prefix = '') {
  for (const key of Object.keys(enObj)) {
    const next = prefix ? `${prefix}.${key}` : key;
    if (typeof enObj[key] === 'object' && enObj[key] !== null && !Array.isArray(enObj[key])) {
      walk(enObj[key], ptObj[key], next);
    } else if (typeof enObj[key] === 'string' && enObj[key] === ptObj[key] && enObj[key].length > 3) {
      sameAsEn.push({ path: next, value: enObj[key] });
    }
  }
}
walk(en, pt);
console.log('identical to en (len>3):', sameAsEn.length);
console.log('sample identical:', sameAsEn.slice(0, 20));

const phRe = /\{\{[^}]+\}\}/g;
let phIssues = 0;
for (const dotPath of enPaths) {
  const enVal = get(en, dotPath);
  const ptVal = get(pt, dotPath);
  const enPh = [...enVal.matchAll(phRe)].map((m) => m[0]).sort().join(',');
  const ptPh = [...ptVal.matchAll(phRe)].map((m) => m[0]).sort().join(',');
  if (enPh !== ptPh) {
    phIssues += 1;
    if (phIssues <= 5) console.log('placeholder issue', dotPath, enPh, ptPh);
  }
}
console.log('placeholder issues:', phIssues);
