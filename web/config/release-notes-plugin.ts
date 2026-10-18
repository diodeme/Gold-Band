import { existsSync, readFileSync } from 'node:fs';
import path from 'node:path';
import { normalizePath, type Plugin } from 'vite';

export const RELEASE_NOTES_MODULE_ID = 'virtual:gold-band-release-notes';
const RESOLVED_RELEASE_NOTES_MODULE_ID = `\0${RELEASE_NOTES_MODULE_ID}`;
const RELEASE_NOTES_LOCALES = ['zh-CN', 'zh-TW', 'en', 'ja-JP', 'ko-KR', 'pt-BR', 'es'] as const;

/**
 * Embeds `release-notes/<package.json version>/<locale>.md` for the version being built, one lazy
 * chunk per locale, so the app can show its own release notes offline without loading them at startup.
 * Every channel uses the release notes of its base version.
 */
export function releaseNotesModuleSource(repoRoot: string) {
  const { version } = JSON.parse(readFileSync(path.join(repoRoot, 'package.json'), 'utf8')) as { version: string };
  const versionDir = path.join(repoRoot, 'release-notes', version);
  const loaders = RELEASE_NOTES_LOCALES
    .map((locale) => ({ locale, file: path.join(versionDir, `${locale}.md`) }))
    .filter(({ file }) => existsSync(file))
    .map(({ locale, file }) => `  ${JSON.stringify(locale)}: () => import(${JSON.stringify(`${normalizePath(file)}?raw`)}).then((module) => module.default),`);
  return [
    `export const releaseNotesVersion = ${JSON.stringify(version)};`,
    'export const releaseNotesLoaders = {',
    ...loaders,
    '};',
    '',
  ].join('\n');
}

export function releaseNotesPlugin(repoRoot: string): Plugin {
  return {
    name: 'gold-band-release-notes',
    resolveId(id) {
      return id === RELEASE_NOTES_MODULE_ID ? RESOLVED_RELEASE_NOTES_MODULE_ID : undefined;
    },
    load(id) {
      if (id !== RESOLVED_RELEASE_NOTES_MODULE_ID) return undefined;
      this.addWatchFile(path.join(repoRoot, 'package.json'));
      return releaseNotesModuleSource(repoRoot);
    },
  };
}
