/// <reference types="vite/client" />

declare module 'virtual:gold-band-release-notes' {
  /** Version whose release notes were embedded at build time (the package base version). */
  export const releaseNotesVersion: string;
  export const releaseNotesLoaders: Partial<Record<string, () => Promise<string>>>;
}
