import { readdirSync, readFileSync } from 'node:fs';
import { extname, join, relative, resolve } from 'node:path';
import ts from 'typescript';
import { describe, expect, it } from 'vitest';

const sourceRoot = resolve(process.cwd(), 'web/src');
const nativeTitleForwarders = new Set(['Button', 'Handle']);

function sourceFiles(directory: string): string[] {
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const path = join(directory, entry.name);
    if (entry.isDirectory()) return sourceFiles(path);
    return ['.tsx', '.jsx'].includes(extname(entry.name)) ? [path] : [];
  });
}

function nativeTitleLocations() {
  const locations: string[] = [];
  for (const path of sourceFiles(sourceRoot)) {
    const source = readFileSync(path, 'utf8');
    const file = ts.createSourceFile(path, source, ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);
    const visit = (node: ts.Node) => {
      if (ts.isJsxOpeningElement(node) || ts.isJsxSelfClosingElement(node)) {
        const tagName = node.tagName.getText(file);
        const nativeElement = tagName[0] === tagName[0]?.toLowerCase();
        const forwardsToNativeElement = nativeTitleForwarders.has(tagName);
        const title = node.attributes.properties.find(
          (property) => ts.isJsxAttribute(property) && property.name.getText(file) === 'title',
        );
        if (title && (nativeElement || forwardsToNativeElement)) {
          const position = file.getLineAndCharacterOfPosition(title.getStart(file));
          locations.push(`${relative(process.cwd(), path)}:${position.line + 1}`);
        }
      }
      ts.forEachChild(node, visit);
    };
    visit(file);
  }
  return locations;
}

describe('visual tooltip contract', () => {
  it('does not use browser-native title attributes for product tooltips', () => {
    expect(nativeTitleLocations()).toEqual([]);
  });
});
