import { readdirSync, readFileSync } from 'node:fs';
import { extname, join, relative, resolve } from 'node:path';
import ts from 'typescript';
import { describe, expect, it } from 'vitest';

const sourceRoot = resolve(process.cwd(), 'web/src');
const tooltipSourcePath = join(sourceRoot, 'components', 'ui', 'tooltip.tsx');
const stylesSourcePath = join(sourceRoot, 'styles.css');
const composerOverflowTooltipSources = [
  join(sourceRoot, 'components', 'acp', 'AcpSingleConfigMenu.tsx'),
  join(sourceRoot, 'components', 'acp', 'AcpModelThoughtSelects.tsx'),
];
const overflowClassPattern = /(?:^|[\s"'`])overflow-(?:auto|y-auto|y-scroll)(?:$|[\s"'`])/;

function sourceFiles(directory: string): string[] {
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const path = join(directory, entry.name);
    if (entry.isDirectory()) return sourceFiles(path);
    return ['.tsx', '.jsx'].includes(extname(path)) ? [path] : [];
  });
}

function stringLiterals(node: ts.Node): string[] {
  const values: string[] = [];
  const visit = (child: ts.Node) => {
    if (ts.isStringLiteral(child) || ts.isNoSubstitutionTemplateLiteral(child)) {
      values.push(child.text);
    }
    ts.forEachChild(child, visit);
  };
  visit(node);
  return values;
}

function jsxClassName(element: ts.JsxOpeningLikeElement, file: ts.SourceFile) {
  const attribute = element.attributes.properties.find(
    (property) => ts.isJsxAttribute(property) && property.name.getText(file) === 'className',
  );
  if (!attribute || !ts.isJsxAttribute(attribute) || !attribute.initializer) return '';
  return stringLiterals(attribute.initializer).join(' ');
}

function hasScrollOverflow(classText: string) {
  return overflowClassPattern.test(classText);
}

function jsxTreeHasScrollOverflow(node: ts.Node, file: ts.SourceFile): boolean {
  if (ts.isJsxSelfClosingElement(node)) return hasScrollOverflow(jsxClassName(node, file));
  if (ts.isJsxElement(node)) {
    if (hasScrollOverflow(jsxClassName(node.openingElement, file))) return true;
    return node.children.some((child) => jsxTreeHasScrollOverflow(child, file));
  }
  if (ts.isJsxExpression(node) && node.expression) {
    return jsxTreeHasScrollOverflow(node.expression, file);
  }
  let found = false;
  ts.forEachChild(node, (child) => {
    if (!found && jsxTreeHasScrollOverflow(child, file)) found = true;
  });
  return found;
}

function scrollableTooltipContentWithoutPointerOptIn() {
  const locations: string[] = [];
  for (const path of sourceFiles(sourceRoot)) {
    const source = readFileSync(path, 'utf8');
    const file = ts.createSourceFile(path, source, ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);
    const visit = (node: ts.Node) => {
      if (ts.isJsxOpeningElement(node) || ts.isJsxSelfClosingElement(node)) {
        if (node.tagName.getText(file) !== 'TooltipContent') {
          ts.forEachChild(node, visit);
          return;
        }
        const tree = ts.isJsxOpeningElement(node) && ts.isJsxElement(node.parent) ? node.parent : node;
        const classes = jsxClassName(node, file);
        if (jsxTreeHasScrollOverflow(tree, file) && !classes.includes('pointer-events-auto')) {
          const position = file.getLineAndCharacterOfPosition(node.tagName.getStart(file));
          locations.push(`${relative(process.cwd(), path)}:${position.line + 1}`);
        }
      }
      ts.forEachChild(node, visit);
    };
    visit(file);
  }
  return locations;
}

describe('Tooltip pointer-events contract', () => {
  it('keeps shared TooltipContent from participating in hit-testing by default', () => {
    const source = readFileSync(tooltipSourcePath, 'utf8');
    const file = ts.createSourceFile(tooltipSourcePath, source, ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);
    const defaultClasses: string[] = [];
    const visit = (node: ts.Node) => {
      if (ts.isFunctionDeclaration(node) && node.name?.text === 'TooltipContent') {
        defaultClasses.push(...stringLiterals(node));
      }
      ts.forEachChild(node, visit);
    };
    visit(file);
    expect(defaultClasses.join(' ')).toContain('pointer-events-none');
  });

  it('requires scrollable tooltip content to opt back into pointer events', () => {
    expect(scrollableTooltipContentWithoutPointerOptIn()).toEqual([]);
  });

  it('keeps the Radix tooltip popper wrapper out of hit-testing', () => {
    const styles = readFileSync(stylesSourcePath, 'utf8');
    expect(styles).toMatch(
      /\[data-radix-popper-content-wrapper\]:has\(\s*>\s*\[data-slot=["']tooltip-content["']\]\s*\)\s*\{[^}]*pointer-events:\s*none/s,
    );
    expect(styles).not.toMatch(
      /\[data-radix-popper-content-wrapper\]:has\(\s*>\s*\[data-slot=["'](?:dropdown-menu|popover)-content/s,
    );
  });

  it('anchors composer overflow tooltips to the whole trigger, not the truncated value', () => {
    const dropdown = readFileSync(join(sourceRoot, 'components', 'ui', 'dropdown-menu.tsx'), 'utf8');
    expect(dropdown).toMatch(/\{\.\.\.props\}\s*data-slot="dropdown-menu-trigger"/s);
    for (const path of composerOverflowTooltipSources) {
      const source = readFileSync(path, 'utf8');
      expect(source, relative(process.cwd(), path)).toMatch(/<TooltipTrigger asChild>\s*<DropdownMenuTrigger/s);
      expect(source, relative(process.cwd(), path)).not.toMatch(
        /<TooltipTrigger asChild>\s*<span[\s\S]*?data-acp-config-value/s,
      );
    }
  });
});
