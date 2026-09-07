import { readFileSync } from 'node:fs';
import path from 'node:path';
import { describe, expect, it } from 'vitest';

const LOOKBEHIND_SOURCE = /\(\?<(?:=|!)/u;

function readProject(relativePath: string) {
  return readFileSync(path.resolve(__dirname, '..', '..', relativePath), 'utf8');
}

describe('WebKit 613 RegExp compatibility', () => {
  it('keeps the production Markdown execution chain free of native lookbehind', () => {
    const runtimeSources = [
      readProject('web/src/components/prompt-kit/markdown.tsx'),
      readProject('node_modules/remend/dist/index.js'),
      readProject('node_modules/mdast-util-gfm-autolink-literal/lib/index.js'),
    ];

    for (const source of runtimeSources) {
      expect(source).not.toMatch(LOOKBEHIND_SOURCE);
    }
  });
});
