import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { afterEach, describe, expect, it } from 'vitest';
import { releaseNotesModuleSource } from '../config/release-notes-plugin';

const roots: string[] = [];

afterEach(() => {
  for (const root of roots.splice(0)) rmSync(root, { recursive: true, force: true });
});

function repo(version: string, locales: string[]) {
  const root = mkdtempSync(path.join(os.tmpdir(), 'gold-band-release-notes-'));
  roots.push(root);
  writeFileSync(path.join(root, 'package.json'), JSON.stringify({ version }));
  if (locales.length) {
    const dir = path.join(root, 'release-notes', version);
    mkdirSync(dir, { recursive: true });
    for (const locale of locales) writeFileSync(path.join(dir, `${locale}.md`), `# ${locale}`);
  }
  return root;
}

describe('release notes build module', () => {
  it('embeds only the package version notes as one lazy raw chunk per existing locale', () => {
    const root = repo('1.2.3', ['zh-CN', 'en']);
    mkdirSync(path.join(root, 'release-notes', '1.2.2'), { recursive: true });
    writeFileSync(path.join(root, 'release-notes', '1.2.2', 'ja-JP.md'), '# old');
    const source = releaseNotesModuleSource(root);
    expect(source).toContain('export const releaseNotesVersion = "1.2.3";');
    const imports = [...source.matchAll(/"([^"]+)": \(\) => import\("([^"]+)"\)/g)].map(([, locale, file]) => [locale, file]);
    expect(imports.map(([locale]) => locale)).toEqual(['zh-CN', 'en']);
    for (const [locale, file] of imports) {
      expect(file.endsWith(`/release-notes/1.2.3/${locale}.md?raw`)).toBe(true);
      expect(file).not.toContain('\\');
    }
    expect(source).not.toContain('1.2.2');
  });

  it('exposes no loaders when the version has no release notes', () => {
    const source = releaseNotesModuleSource(repo('9.9.9', []));
    expect(source).toContain('export const releaseNotesVersion = "9.9.9";');
    expect(source).not.toContain('import(');
  });
});
