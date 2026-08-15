import { createHash } from 'node:crypto';
import { lstat, mkdir, readFile, readdir, realpath, stat, writeFile } from 'node:fs/promises';
import { dirname, extname, join, relative, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';

import Ajv from 'ajv';
import StyleDictionary from 'style-dictionary';

import {
  RECIPE_ROLE_NAMES,
  SEMANTIC_TOKEN_NAMES,
  runtimeThemeSchema,
  themeManifestSchema,
} from './src/contract.mjs';

const sdkRoot = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(sdkRoot, '..');
const themesRoot = join(repoRoot, 'themes');
const generatedWebRoot = join(repoRoot, 'web', 'src', 'themes', 'generated');
const rustCatalogPath = join(repoRoot, 'resources', 'themes', 'builtin-theme-catalog.json');
const schemaRoot = join(sdkRoot, 'schema');
const ajv = new Ajv({ allErrors: true, strict: true });
const validateManifest = ajv.compile(themeManifestSchema);
const validateRuntimeTheme = ajv.compile(runtimeThemeSchema);

await Promise.all([
  mkdir(schemaRoot, { recursive: true }),
  mkdir(generatedWebRoot, { recursive: true }),
  mkdir(dirname(rustCatalogPath), { recursive: true }),
]);
await Promise.all([
  writeJson(join(schemaRoot, 'theme-manifest-v2.schema.json'), themeManifestSchema),
  writeJson(join(schemaRoot, 'theme-package-v2.schema.json'), runtimeThemeSchema),
]);

const themeDirectories = (await readdir(themesRoot, { withFileTypes: true }))
  .filter((entry) => entry.isDirectory() && !entry.name.startsWith('.') && entry.name !== 'dist')
  .map((entry) => join(themesRoot, entry.name))
  .sort();
const packages = [];
for (const themeDirectory of themeDirectories) {
  packages.push(await buildTheme(themeDirectory));
}
packages.sort((left, right) => {
  if (left.id === 'builtin.gold-band') return -1;
  if (right.id === 'builtin.gold-band') return 1;
  return left.id.localeCompare(right.id);
});

const ids = new Set();
for (const themePackage of packages) {
  if (ids.has(themePackage.id)) throw new Error(`Duplicate theme id: ${themePackage.id}`);
  ids.add(themePackage.id);
}
if (!ids.has('builtin.gold-band')) throw new Error('Safe fallback theme builtin.gold-band is required');

await Promise.all([
  writeJson(join(generatedWebRoot, 'catalog.json'), packages),
  writeJson(rustCatalogPath, packages),
  writeFile(join(generatedWebRoot, 'builtin-themes.css'), `${packages.map(compilePackageCss).join('\n')}\n`, 'utf8'),
]);

console.log(`Built ${packages.length} declarative theme packages: ${packages.map((theme) => theme.id).join(', ')}`);

async function buildTheme(themeDirectory) {
  const manifest = await readJson(join(themeDirectory, 'manifest.json'));
  assertSchema(validateManifest, manifest, `${manifest.id ?? themeDirectory} manifest`);
  const declaresProfiles = manifest.capabilities.includes('visual-quality-profiles');
  if (declaresProfiles !== Boolean(manifest.visualQualityProfiles)) {
    throw new Error(`${manifest.id}: visual-quality-profiles capability and manifest configuration must be declared together`);
  }

  const [recipes, presets, light, dark, assets] = await Promise.all([
    readJson(join(themeDirectory, 'recipes.json')),
    readJson(join(themeDirectory, 'presets.json')),
    resolveSchemeTokens(themeDirectory, 'light'),
    resolveSchemeTokens(themeDirectory, 'dark'),
    buildAssetManifest(themeDirectory),
  ]);
  const visualQualityProfiles = declaresProfiles
    ? {
        default: manifest.visualQualityProfiles.default,
        supported: manifest.visualQualityProfiles.supported,
        performance: await readPackageJson(themeDirectory, manifest.visualQualityProfiles.performance),
      }
    : undefined;
  const themePackage = {
    schemaVersion: manifest.schemaVersion,
    contractVersion: manifest.contractVersion,
    id: manifest.id,
    version: manifest.version,
    source: manifest.source,
    name: manifest.name,
    author: manifest.author,
    capabilities: manifest.capabilities,
    schemes: {
      light: { ...light, ...presets.light },
      dark: { ...dark, ...presets.dark },
    },
    recipes,
    ...(visualQualityProfiles ? { visualQualityProfiles } : {}),
  };
  assertSchema(validateRuntimeTheme, themePackage, `${manifest.id} runtime package`);
  assertRecipeCoverage(themePackage);
  assertFontStackUniqueness(themePackage);

  const dist = join(themeDirectory, 'dist');
  await mkdir(dist, { recursive: true });
  await Promise.all([
    writeJson(join(dist, 'runtime-theme.json'), themePackage),
    writeJson(join(dist, 'asset-manifest.json'), assets),
    writeFile(join(dist, 'builtin-theme.css'), `${compilePackageCss(themePackage)}\n`, 'utf8'),
  ]);
  return themePackage;
}

async function resolveSchemeTokens(themeDirectory, scheme) {
  const source = [
    join(themeDirectory, 'tokens', 'primitives.tokens.json'),
    join(themeDirectory, 'tokens', 'semantic.tokens.json'),
    join(themeDirectory, 'tokens', `${scheme}.tokens.json`),
  ];
  const dictionary = new StyleDictionary({
    source,
    usesDtcg: true,
    log: { verbosity: 'silent' },
    platforms: { runtime: { transforms: [] } },
  });
  const resolved = await dictionary.getPlatformTokens('runtime');
  const values = stripTokenMetadata(resolved.tokens);
  return {
    windowSurface: values.windowSurface,
    preview: values.preview,
    semantic: values.semantic,
    material: normalizeMaterial(values.material),
  };
}

function normalizeMaterial(material) {
  return {
    model: 'solid',
    backdropBrightness: 100,
    backdropContrast: 100,
    specularHighlight: 'none',
    edgeShadow: '0 0 0 transparent',
    ...material,
  };
}

function stripTokenMetadata(node) {
  if (node && typeof node === 'object' && '$value' in node) return node.$value;
  if (!node || typeof node !== 'object' || Array.isArray(node)) return node;
  return Object.fromEntries(Object.entries(node)
    .filter(([key]) => !key.startsWith('$'))
    .map(([key, value]) => [key, stripTokenMetadata(value)]));
}

async function buildAssetManifest(themeDirectory) {
  const assetsRoot = join(themeDirectory, 'assets');
  try {
    await stat(assetsRoot);
  } catch (error) {
    if (error?.code === 'ENOENT') return { schemaVersion: 1, assets: [] };
    throw error;
  }
  const files = [];
  await walkAssets(assetsRoot, assetsRoot, files);
  if (files.length > 128) throw new Error(`${themeDirectory}: theme asset count exceeds 128`);
  const totalBytes = files.reduce((sum, asset) => sum + asset.bytes, 0);
  if (totalBytes > 16 * 1024 * 1024) throw new Error(`${themeDirectory}: theme assets exceed 16 MiB`);
  return { schemaVersion: 1, assets: files };
}

async function walkAssets(root, directory, files) {
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const path = join(directory, entry.name);
    const metadata = await lstat(path);
    if (metadata.isSymbolicLink()) throw new Error(`Theme assets cannot contain symbolic links: ${path}`);
    if (entry.isDirectory()) {
      await walkAssets(root, path, files);
      continue;
    }
    const extension = extname(entry.name).toLowerCase();
    if (!['.png', '.webp', '.woff', '.woff2'].includes(extension)) {
      throw new Error(`Unsupported theme asset type: ${path}`);
    }
    const data = await readFile(path);
    files.push({
      path: relative(root, path).split(sep).join('/'),
      bytes: data.byteLength,
      sha256: createHash('sha256').update(data).digest('hex'),
      mediaType: mediaType(extension),
    });
  }
}

async function readPackageJson(themeDirectory, packageRelativePath) {
  const target = resolve(themeDirectory, packageRelativePath);
  const themeRoot = await realpath(themeDirectory);
  const targetParent = await realpath(dirname(target));
  if (targetParent !== themeRoot && !targetParent.startsWith(`${themeRoot}${sep}`)) {
    throw new Error(`Theme path escapes package root: ${packageRelativePath}`);
  }
  return readJson(target);
}

function assertRecipeCoverage(themePackage) {
  for (const role of RECIPE_ROLE_NAMES) {
    if (!themePackage.recipes[role]) throw new Error(`${themePackage.id}: missing recipe role ${role}`);
  }
}

function compilePackageCss(themePackage) {
  const themeSelector = `:root[data-theme='${themePackage.id}']`;
  const blocks = Object.entries(themePackage.schemes).map(([schemeName, scheme]) => {
    const declarations = Object.entries(scheme.semantic)
      .map(([name, value]) => `${semanticCssVariable(name)}:${value}`);
    declarations.push(
      `--radius:${scheme.material.radius}`,
      `--gb-material-model:${scheme.material.model}`,
      `--gb-material-opacity:${scheme.material.surfaceOpacity}`,
      `--gb-material-border-highlight:${scheme.material.borderHighlight}`,
      `--gb-material-surface-overlay:${scheme.material.surfaceOverlay}`,
      `--gb-material-blur:${scheme.material.blur}px`,
      `--gb-material-saturate:${scheme.material.saturate}%`,
      `--gb-material-backdrop-brightness:${scheme.material.backdropBrightness}%`,
      `--gb-material-backdrop-contrast:${scheme.material.backdropContrast}%`,
      `--gb-material-specular-highlight:${scheme.material.specularHighlight}`,
      `--gb-material-edge-shadow:${scheme.material.edgeShadow}`,
      `--gb-material-shadow:${scheme.material.shadow}`,
      `--gb-theme-background-image:${scheme.material.backgroundImage}`,
      `--gb-theme-texture-opacity:${scheme.material.textureOpacity}`,
      `--gb-theme-motion-duration:${scheme.material.motionDuration}`,
      `--gb-theme-motion-easing:${scheme.material.motionEasing}`,
      `--gb-theme-ui-font-family:${serializeFontStack(scheme.typography.ui)}`,
      `--gb-theme-editor-font-family:${serializeFontStack(scheme.typography.editor)}`,
      `--gb-theme-ui-font-size:${scheme.typography.ui.size}px`,
      `--gb-theme-editor-font-size:${scheme.typography.editor.size}px`,
    );
    return `${themeSelector}[data-color-scheme='${schemeName}']{${declarations.join(';')}}`;
  });
  blocks.push(
    `${themeSelector} body,${themeSelector} #root,${themeSelector} .app-window-shell{background-color:var(--gold-workspace);background-image:var(--gb-theme-background-image);background-attachment:fixed;background-size:cover}`,
    `${themeSelector} .app-window-shell::before{position:fixed;z-index:0;inset:0;pointer-events:none;content:"";opacity:var(--gb-theme-texture-opacity);background-image:repeating-radial-gradient(circle at 17% 31%,currentColor 0 .45px,transparent .55px 3px),repeating-radial-gradient(circle at 73% 61%,currentColor 0 .4px,transparent .5px 4px);mix-blend-mode:soft-light}`,
    `${themeSelector} [data-theme-role='shell'] > *,${themeSelector} [data-theme-role='shell'] main,${themeSelector} [data-theme-role='panel']{position:relative;z-index:1}`,
  );
  if (themePackage.visualQualityProfiles) {
    const performance = themePackage.visualQualityProfiles.performance;
    blocks.push(`${themeSelector}[data-visual-quality='performance']{--gb-material-blur:${performance.blur}px;--gb-material-saturate:${performance.saturate}%;--gb-material-backdrop-brightness:${performance.backdropBrightness ?? 100}%;--gb-material-backdrop-contrast:${performance.backdropContrast ?? 100}%;--gb-material-specular-highlight:${performance.specularHighlight ?? 'none'};--gb-material-edge-shadow:${performance.edgeShadow ?? '0 0 0 transparent'};--gb-material-shadow:${performance.shadow};--gb-theme-texture-opacity:${performance.textureOpacity};--gb-theme-motion-duration:${performance.motionDuration}}`);
  }
  for (const [role, recipe] of Object.entries(themePackage.recipes)) {
    const declarations = [
      `--gb-recipe-background:${backgroundVariable(recipe.background)}`,
      `--gb-recipe-foreground:${foregroundVariable(recipe.foreground)}`,
      `--gb-recipe-border:${borderVariable(recipe.border)}`,
    ];
    if (role !== 'button') declarations.push('background-color:var(--gb-recipe-background)', 'color:var(--gb-recipe-foreground)', 'border-color:var(--gb-recipe-border)');
    if (recipe.material !== 'flat') {
      declarations.push(
        'backdrop-filter:blur(var(--gb-material-blur)) saturate(var(--gb-material-saturate)) brightness(var(--gb-material-backdrop-brightness)) contrast(var(--gb-material-backdrop-contrast))',
        '-webkit-backdrop-filter:blur(var(--gb-material-blur)) saturate(var(--gb-material-saturate)) brightness(var(--gb-material-backdrop-brightness)) contrast(var(--gb-material-backdrop-contrast))',
        recipe.material === 'elevated'
          ? 'box-shadow:var(--gb-material-shadow),var(--gb-material-edge-shadow)'
          : 'box-shadow:var(--gb-material-edge-shadow)',
        'transition-duration:var(--gb-theme-motion-duration)',
        'transition-timing-function:var(--gb-theme-motion-easing)',
      );
    }
    if (recipe.material === 'elevated') {
      declarations.push('background-image:var(--gb-material-surface-overlay)');
    }
    blocks.push(`${themeSelector} [data-theme-role='${role}']{${declarations.join(';')}}`);
  }
  const opticalRoles = Object.entries(themePackage.recipes)
    .filter(([, recipe]) => recipe.material !== 'flat')
    .map(([role]) => `${themeSelector}[data-material-model='liquid'] [data-theme-role='${role}']`);
  if (opticalRoles.length > 0) {
    blocks.push(`${opticalRoles.join(',')}{background-image:var(--gb-material-specular-highlight),var(--gb-material-surface-overlay);background-blend-mode:screen,normal}`);
  }
  blocks.push(`${themeSelector}{--gb-theme-package-version:'${themePackage.version}'}`);
  blocks.push(`@media (prefers-reduced-motion:reduce){${themeSelector} [data-theme-role]{transition-duration:.01ms!important}}`);
  return blocks.join('\n');
}

function assertFontStackUniqueness(themePackage) {
  for (const [schemeName, scheme] of Object.entries(themePackage.schemes)) {
    for (const [kind, stack] of Object.entries(scheme.typography)) {
      const normalized = stack.families.map((family) => family.toLocaleLowerCase());
      if (new Set(normalized).size !== normalized.length) {
        throw new Error(`${themePackage.id}: ${schemeName} ${kind} font families contain a case-insensitive duplicate`);
      }
    }
  }
}

function serializeFontStack(stack) {
  return [...stack.families.map(quoteFontFamily), stack.fallback].join(', ');
}

function quoteFontFamily(family) {
  return `"${family.replaceAll('\\', '\\\\').replaceAll('"', '\\"')}"`;
}

function semanticCssVariable(name) {
  const kebab = name.replace(/[A-Z]/gu, (letter) => `-${letter.toLowerCase()}`);
  if (name === 'selection') return '--text-selection';
  if (name === 'selectionForeground') return '--text-selection-foreground';
  if (name === 'messageUser' || name === 'messageUserForeground') return `--${kebab}`;
  if (name.startsWith('sidebar')) return `--${kebab}`;
  if (name.startsWith('titlebar')) return `--${kebab}`;
  if (name.startsWith('scrollbar')) return `--gold-${kebab}`;
  if (['workspace', 'surfaceLow', 'surfaceHigh', 'lineSoft', 'windowOutline', 'windowEdgeShadow', 'running', 'success', 'warning', 'danger', 'permission'].includes(name)) return `--gold-${kebab}`;
  if (['contentHeader', 'contentHeaderForeground', 'conversationBackground', 'conversationForeground', 'messageAssistant', 'messageAssistantForeground', 'composer', 'composerForeground', 'activity', 'activityForeground', 'toolCard', 'toolCardForeground', 'permissionCard', 'permissionCardForeground', 'workspaceTab', 'workspaceTabForeground', 'resourceHeader', 'resourceHeaderForeground', 'fileTree', 'fileTreeForeground', 'editor', 'editorForeground', 'diffAdded', 'diffAddedForeground', 'diffRemoved', 'diffRemovedForeground', 'diffModified', 'diffModifiedForeground'].includes(name)) return `--gb-${kebab}`;
  return `--${kebab}`;
}

function backgroundVariable(value) {
  if (value === 'transparent') return 'transparent';
  const names = { card: '--card', popover: '--popover', sidebar: '--sidebar', 'surface-low': '--gold-surface-low', 'surface-high': '--gold-surface-high' };
  return `var(${names[value]})`;
}

function foregroundVariable(value) {
  const names = { foreground: '--foreground', 'muted-foreground': '--muted-foreground', 'card-foreground': '--card-foreground' };
  return `var(${names[value]})`;
}

function borderVariable(value) {
  const names = { border: '--border', 'sidebar-border': '--sidebar-border', highlight: '--gb-material-border-highlight' };
  return `var(${names[value]})`;
}

function mediaType(extension) {
  return { '.png': 'image/png', '.webp': 'image/webp', '.woff': 'font/woff', '.woff2': 'font/woff2' }[extension];
}

function assertSchema(validate, value, label) {
  if (validate(value)) return;
  const details = validate.errors?.map((error) => `${error.instancePath || '/'} ${error.message}`).join('; ');
  throw new Error(`${label} is invalid: ${details}`);
}

async function readJson(path) {
  return JSON.parse(await readFile(path, 'utf8'));
}

async function writeJson(path, value) {
  await writeFile(path, `${JSON.stringify(value, null, 2)}\n`, 'utf8');
}
