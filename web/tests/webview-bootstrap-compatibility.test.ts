import { existsSync, readFileSync } from 'node:fs';
import path from 'node:path';
import { describe, expect, it } from 'vitest';
import viteConfig from '../vite.config';

describe('WebView compatibility bootstrap', () => {
  it('gates the business application behind a standalone preflight entry', () => {
    const html = readFileSync(path.resolve(__dirname, '../index.html'), 'utf8');
    const bootstrapPath = path.resolve(__dirname, '../src/webview-bootstrap.ts');

    expect(html).toContain('/src/webview-bootstrap.ts');
    expect(html).not.toContain('/src/main.tsx');
    expect(existsSync(bootstrapPath)).toBe(true);

    const bootstrap = readFileSync(bootstrapPath, 'utf8');
    expect(bootstrap).not.toMatch(/from ['"](?:react|react-dom|\.\/App|\.\/main)['"]/u);
    expect(bootstrap).toContain("import('./main')");

    const main = readFileSync(path.resolve(__dirname, '../src/main.tsx'), 'utf8');
    expect(main).toContain("import './webview-compatibility.css'");
  });

  it('builds application chunks for the documented Safari 15.4 baseline', () => {
    expect(viteConfig).toMatchObject({
      build: {
        target: 'safari15.4',
      },
    });
  });
});
