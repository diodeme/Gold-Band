import { releaseNotesLoaders, releaseNotesVersion } from 'virtual:gold-band-release-notes';

export { releaseNotesVersion };

/** zh-CN is the authoring source of release notes, so it covers locales without a translation. */
const FALLBACK_RELEASE_NOTES_LOCALE = 'zh-CN';

/** Loads the embedded notes of the running version; `null` means this build has none. */
export function loadReleaseNotes(locale: string): Promise<string | null> {
  const loader = releaseNotesLoaders[locale] ?? releaseNotesLoaders[FALLBACK_RELEASE_NOTES_LOCALE];
  return loader ? loader() : Promise.resolve(null);
}
