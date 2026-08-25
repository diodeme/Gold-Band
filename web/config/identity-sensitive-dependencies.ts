/**
 * Packages whose runtime contracts depend on shared module identity.
 *
 * Keep Vite and Vitest on the same list so a strict package-manager layout
 * cannot split React contexts or CodeMirror/Lezer facets across module copies.
 */
export const identitySensitiveDependencies = [
  'react',
  'react-dom',
  '@codemirror/autocomplete',
  '@codemirror/commands',
  '@codemirror/lang-markdown',
  '@codemirror/state',
  '@codemirror/view',
  '@codemirror/language',
  '@codemirror/search',
  '@lezer/common',
  '@lezer/highlight',
] as const;
