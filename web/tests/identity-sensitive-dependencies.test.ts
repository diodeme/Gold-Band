import { describe, expect, it } from 'vitest';
import viteConfig from '../vite.config';
import { identitySensitiveDependencies } from '../config/identity-sensitive-dependencies';

describe('identity-sensitive dependency resolution', () => {
  it('keeps the production Vite build on the shared singleton contract', () => {
    expect(identitySensitiveDependencies).toEqual([
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
    ]);
    expect(viteConfig).toMatchObject({
      resolve: {
        dedupe: [...identitySensitiveDependencies],
      },
    });
  });
});
