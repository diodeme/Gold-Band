import { createHash, randomUUID } from 'node:crypto';
import { copyFile, lstat, mkdir, readFile, readdir, realpath, rename, rm, stat, writeFile } from 'node:fs/promises';
import { basename, dirname, extname, join, relative, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';

import Ajv from 'ajv';
import * as fontkit from 'fontkit';
import { imageSize } from 'image-size';
import StyleDictionary from 'style-dictionary';

import {
  MAX_THEME_ASSETS,
  MAX_THEME_ASSET_BYTES,
  RECIPE_ROLE_NAMES,
  SEMANTIC_TOKEN_NAMES,
  THEME_ICON_SLOTS,
  THEME_VISUAL_STATES,
  THEME_WALLPAPER_SLOTS,
  runtimeThemeSchema,
  themeAssetSourceSchema,
  themeManifestSchema,
} from './src/contract.mjs';

const sdkRoot = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(sdkRoot, '..');
const themesRoot = join(repoRoot, 'themes');
const generatedWebRoot = join(repoRoot, 'web', 'src', 'themes', 'generated');
const generatedAssetRoot = join(repoRoot, 'web', 'public', 'theme-assets');
const rustCatalogPath = join(repoRoot, 'resources', 'themes', 'builtin-theme-catalog.json');
const schemaRoot = join(sdkRoot, 'schema');
const stageRoot = join(repoRoot, `.theme-build-${randomUUID()}`);
const ajv = new Ajv({ allErrors: true, strict: true });
const validateManifest = ajv.compile(themeManifestSchema);
const validateRuntimeTheme = ajv.compile(runtimeThemeSchema);
const validateAssetSource = ajv.compile(themeAssetSourceSchema);

try {
  await mkdir(stageRoot, { recursive: true });
  const themeDirectories = (await readdir(themesRoot, { withFileTypes: true }))
    .filter((entry) => entry.isDirectory() && !entry.name.startsWith('.') && entry.name !== 'dist')
    .map((entry) => join(themesRoot, entry.name))
    .sort();
  const packages = [];
  const stagedThemeOutputs = [];
  for (const themeDirectory of themeDirectories) {
    const result = await buildTheme(themeDirectory);
    packages.push(result.themePackage);
    stagedThemeOutputs.push(result);
  }
  packages.sort((left, right) => left.id === 'builtin.gold-band' ? -1 : right.id === 'builtin.gold-band' ? 1 : left.id.localeCompare(right.id));
  assertUnique(packages.map((theme) => theme.id), 'theme id');
  if (!packages.some((theme) => theme.id === 'builtin.gold-band')) throw themeError('theme.active-package-missing', 'builtin.gold-band is required');

  const stagedGeneratedWeb = join(stageRoot, 'web-generated');
  const stagedSchemas = join(stageRoot, 'schemas');
  const stagedRustCatalog = join(stageRoot, 'builtin-theme-catalog.json');
  await Promise.all([mkdir(stagedGeneratedWeb, { recursive: true }), mkdir(stagedSchemas, { recursive: true })]);
  await Promise.all([
    writeJson(join(stagedGeneratedWeb, 'catalog.json'), packages),
    writeJson(join(stagedGeneratedWeb, 'asset-catalog.json'), Object.fromEntries(packages.map((theme) => [theme.id, theme.assets]))),
    writeFile(join(stagedGeneratedWeb, 'builtin-themes.css'), `${packages.map(compilePackageCss).join('\n')}\n`, 'utf8'),
    writeJson(join(stagedSchemas, 'theme-manifest-v2.schema.json'), themeManifestSchema),
    writeJson(join(stagedSchemas, 'theme-package-v2.schema.json'), runtimeThemeSchema),
    writeJson(stagedRustCatalog, packages),
  ]);

  for (const output of stagedThemeOutputs) await replaceDirectory(output.stageDist, join(output.themeDirectory, 'dist'));
  const assetSnapshot = await publishAssetSnapshot(join(stageRoot, 'theme-assets'), generatedAssetRoot);
  await replaceDirectory(stagedGeneratedWeb, generatedWebRoot);
  await replaceDirectory(stagedSchemas, schemaRoot);
  await replaceFile(stagedRustCatalog, rustCatalogPath);
  await pruneAssetSnapshot(generatedAssetRoot, assetSnapshot);
  console.log(`Built ${packages.length} Theme Contract v2 packages: ${packages.map((theme) => theme.id).join(', ')}`);
} finally {
  await rm(stageRoot, { recursive: true, force: true });
}

async function buildTheme(themeDirectory) {
  const manifest = await readJson(join(themeDirectory, 'manifest.json'));
  assertSchema(validateManifest, manifest, `${manifest.id ?? themeDirectory} manifest`, 'theme.package-invalid');
  await assertCapabilityFiles(themeDirectory, manifest);
  const [recipes, presets, light, dark, assetManifest] = await Promise.all([
    readJson(join(themeDirectory, 'recipes.json')),
    readJson(join(themeDirectory, 'presets.json')),
    resolveSchemeTokens(themeDirectory, 'light'),
    resolveSchemeTokens(themeDirectory, 'dark'),
    buildAssetManifest(themeDirectory, manifest.id),
  ]);
  const assetsById = new Map(assetManifest.records.map((asset) => [asset.id, asset]));
  const fonts = manifest.capabilities.includes('fonts') ? await readJson(join(themeDirectory, 'fonts.json')) : undefined;
  const icons = manifest.capabilities.includes('icons') ? await readJson(join(themeDirectory, 'icons.json')) : undefined;
  const wallpapers = manifest.capabilities.includes('wallpapers') ? await readJson(join(themeDirectory, 'wallpapers.json')) : undefined;
  validateAssetReferences({ manifest, presets, fonts, icons, wallpapers, assetsById });
  const runtimeFonts = fonts ? buildRuntimeFonts(fonts, manifest.id) : undefined;
  const visualQualityProfiles = manifest.capabilities.includes('visual-quality-profiles')
    ? { default: manifest.visualQualityProfiles.default, supported: manifest.visualQualityProfiles.supported, performance: await readPackageJson(themeDirectory, manifest.visualQualityProfiles.performance) }
    : undefined;
  const themePackage = {
    schemaVersion: 2, contractVersion: 2, id: manifest.id, version: manifest.version, source: manifest.source,
    name: manifest.name, author: manifest.author, capabilities: manifest.capabilities,
    schemes: { light: { ...light, ...presets.light }, dark: { ...dark, ...presets.dark } },
    recipes, assets: assetManifest, ...(runtimeFonts ? { fonts: runtimeFonts } : {}), ...(icons ? { icons } : {}),
    ...(wallpapers ? { wallpapers } : {}), ...(visualQualityProfiles ? { visualQualityProfiles } : {}),
  };
  assertSchema(validateRuntimeTheme, themePackage, `${manifest.id} runtime package`, 'theme.package-invalid');
  validateRecipeInvariants(themePackage);
  const stageDist = join(stageRoot, 'theme-dist', basename(themeDirectory));
  await mkdir(stageDist, { recursive: true });
  await Promise.all([
    writeJson(join(stageDist, 'runtime-theme.json'), themePackage),
    writeJson(join(stageDist, 'asset-manifest.json'), assetManifest),
    writeFile(join(stageDist, 'builtin-theme.css'), `${compilePackageCss(themePackage)}\n`, 'utf8'),
  ]);
  return { themeDirectory, stageDist, themePackage };
}

async function assertCapabilityFiles(themeDirectory, manifest) {
  const declarations = [
    ['fonts', 'fonts.json'], ['icons', 'icons.json'], ['wallpapers', 'wallpapers.json'],
  ];
  for (const [capability, file] of declarations) {
    const exists = await pathExists(join(themeDirectory, file));
    if (manifest.capabilities.includes(capability) !== exists) throw themeError('theme.capability-file-mismatch', `${manifest.id}: ${capability} and ${file} must be declared together`);
  }
  const declaresQuality = manifest.capabilities.includes('visual-quality-profiles');
  if (declaresQuality !== Boolean(manifest.visualQualityProfiles)) throw themeError('theme.capability-file-mismatch', `${manifest.id}: visual quality capability mismatch`);
  for (const required of ['tokens', 'component-recipes']) {
    if (!manifest.capabilities.includes(required)) throw themeError('theme.capability-file-mismatch', `${manifest.id}: ${required} is required`);
  }
  if (!(await pathExists(join(themeDirectory, 'resources.json')))) throw themeError('theme.capability-file-mismatch', `${manifest.id}: resources.json is required`);
}

async function resolveSchemeTokens(themeDirectory, scheme) {
  const dictionary = new StyleDictionary({
    source: [join(themeDirectory, 'tokens', 'primitives.tokens.json'), join(themeDirectory, 'tokens', 'semantic.tokens.json'), join(themeDirectory, 'tokens', `${scheme}.tokens.json`)],
    usesDtcg: true, log: { verbosity: 'silent' }, platforms: { runtime: { transforms: [] } },
  });
  const resolved = await dictionary.getPlatformTokens('runtime');
  const values = stripTokenMetadata(resolved.tokens);
  return {
    windowSurface: values.windowSurface, preview: values.preview, semantic: values.semantic,
    material: values.material, shape: values.shape, elevation: values.elevation,
    motion: values.motion, scrollbar: values.scrollbar,
  };
}

function stripTokenMetadata(node) {
  if (node && typeof node === 'object' && '$value' in node) return node.$value;
  if (!node || typeof node !== 'object' || Array.isArray(node)) return node;
  return Object.fromEntries(Object.entries(node).filter(([key]) => !key.startsWith('$')).map(([key, value]) => [key, stripTokenMetadata(value)]));
}

async function buildAssetManifest(themeDirectory, themeId) {
  const resources = await readJson(join(themeDirectory, 'resources.json'));
  const licenses = await readJson(join(themeDirectory, 'LICENSES.json'));
  if (resources.schemaVersion !== 2 || !Array.isArray(resources.assets)) throw themeError('theme.package-invalid', `${themeId}: resources.json must use schemaVersion 2`);
  if (licenses.schemaVersion !== 2 || !Array.isArray(licenses.licenses)) throw themeError('theme.package-invalid', `${themeId}: LICENSES.json must use schemaVersion 2`);
  if (resources.assets.length > MAX_THEME_ASSETS) throw themeError('theme.asset-count-exceeded', `${themeId}: more than ${MAX_THEME_ASSETS} assets`);
  const licenseIds = new Set(licenses.licenses.map((license) => license.id));
  assertUnique(resources.assets.map((asset) => asset.id), `${themeId} asset id`, 'theme.asset-id-duplicate');
  const records = [];
  const physicalPaths = new Set();
  for (const source of resources.assets) {
    assertSchema(validateAssetSource, source, `${themeId} asset ${source.id ?? '<unknown>'}`, 'theme.package-invalid');
    if (!licenseIds.has(source.licenseId)) throw themeError('theme.asset-license-missing', `${themeId}: ${source.id} references ${source.licenseId}`);
    const sourcePath = await resolveAssetPath(themeDirectory, source.path);
    const canonicalPath = (await realpath(sourcePath)).toLocaleLowerCase();
    if (physicalPaths.has(canonicalPath)) throw themeError('theme.asset-id-duplicate', `${themeId}: one physical file cannot have multiple asset ids`);
    physicalPaths.add(canonicalPath);
    const data = await readFile(sourcePath);
    const extension = extname(sourcePath).toLowerCase();
    const mediaType = validateMediaSignature(source.kind, extension, data, sourcePath);
    const sha256 = createHash('sha256').update(data).digest('hex');
    const safeName = basename(sourcePath).replace(/[^a-zA-Z0-9._-]/gu, '-');
    const outputName = `${sha256.slice(0, 16)}-${safeName}`;
    const outputDirectory = join(stageRoot, 'theme-assets', themeId);
    await mkdir(outputDirectory, { recursive: true });
    await copyFile(sourcePath, join(outputDirectory, outputName));
    const record = { id: source.id, kind: source.kind, mediaType, bytes: data.byteLength, sha256, outputUrl: `/theme-assets/${themeId}/${outputName}`, required: source.required ?? true, licenseId: source.licenseId };
    if (['icon', 'texture', 'wallpaper', 'preview', 'avatar'].includes(source.kind)) Object.assign(record, inspectImage(source.kind, data, sourcePath));
    if (source.kind === 'font') record.fontMetadata = inspectFont(data, sourcePath);
    records.push(record);
  }
  const totalBytes = records.reduce((sum, asset) => sum + asset.bytes, 0);
  if (totalBytes > MAX_THEME_ASSET_BYTES) throw themeError('theme.asset-budget-exceeded', `${themeId}: assets exceed 16 MiB`);
  return { schemaVersion: 2, count: records.length, totalBytes, records: records.sort((left, right) => left.id.localeCompare(right.id)) };
}

async function resolveAssetPath(themeDirectory, sourcePath) {
  if (sourcePath.includes('\\') || sourcePath.split('/').includes('..')) throw themeError('theme.asset-path-invalid', `${sourcePath}: path must be normalized under assets/`);
  const target = resolve(themeDirectory, sourcePath);
  const themeRoot = await realpath(themeDirectory);
  if (!target.startsWith(`${themeRoot}${sep}`)) throw themeError('theme.asset-path-invalid', `${sourcePath}: path escapes theme package`);
  const relativeParts = relative(themeRoot, target).split(sep);
  let cursor = themeRoot;
  for (const part of relativeParts) {
    cursor = join(cursor, part);
    const metadata = await lstat(cursor);
    if (metadata.isSymbolicLink()) throw themeError('theme.asset-path-invalid', `${sourcePath}: symbolic links are forbidden`);
  }
  return target;
}

function validateMediaSignature(kind, extension, data, path) {
  const mediaTypes = { '.png': 'image/png', '.webp': 'image/webp', '.woff': 'font/woff', '.woff2': 'font/woff2' };
  const mediaType = mediaTypes[extension];
  const allowed = kind === 'font' ? ['.woff', '.woff2'] : ['.png', '.webp'];
  const signatureMatches = extension === '.png' ? data.subarray(0, 8).equals(Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]))
    : extension === '.webp' ? data.toString('ascii', 0, 4) === 'RIFF' && data.toString('ascii', 8, 12) === 'WEBP'
      : extension === '.woff' ? data.toString('ascii', 0, 4) === 'wOFF'
        : extension === '.woff2' ? data.toString('ascii', 0, 4) === 'wOF2' : false;
  if (!allowed.includes(extension) || !signatureMatches) throw themeError('theme.asset-media-type-invalid', `${path}: kind, extension and signature do not match`);
  return mediaType;
}

function inspectImage(kind, data, path) {
  const dimensions = imageSize(data);
  const width = dimensions.width ?? 0;
  const height = dimensions.height ?? 0;
  const limits = kind === 'icon' ? [128, 128, 256 * 1024] : kind === 'texture' ? [1024, 1024, 2 * 1024 * 1024] : [4096, 4096, 4 * 1024 * 1024];
  if (!width || !height || width > limits[0] || height > limits[1] || width * height > 16_000_000 || data.byteLength > limits[2]) throw themeError('theme.asset-dimensions-exceeded', `${path}: ${width}x${height}, ${data.byteLength} bytes`);
  return { width, height };
}

function inspectFont(data, path) {
  if (data.byteLength > 8 * 1024 * 1024) throw themeError('theme.asset-budget-exceeded', `${path}: font exceeds 8 MiB`);
  let font;
  try { font = fontkit.create(data); } catch (error) { throw themeError('theme.font-metadata-invalid', `${path}: ${error.message}`); }
  const axis = font.variationAxes?.wght;
  const fallbackWeight = Number(font['OS/2']?.usWeightClass ?? 400);
  return {
    family: font.familyName ?? '', subfamily: font.subfamilyName ?? '', postscriptName: font.postscriptName ?? '',
    weightMin: Number(axis?.min ?? fallbackWeight), weightMax: Number(axis?.max ?? fallbackWeight),
  };
}

function validateAssetReferences({ manifest, presets, fonts, icons, wallpapers, assetsById }) {
  const requireAsset = (assetId, kind, code) => {
    const asset = assetsById.get(assetId);
    if (!asset || asset.kind !== kind) throw themeError(code, `${manifest.id}: ${assetId} must reference ${kind}`);
    return asset;
  };
  for (const scheme of ['light', 'dark']) for (const assetId of [presets[scheme]?.avatars?.agentAsset, presets[scheme]?.avatars?.userAsset]) if (assetId) requireAsset(assetId, 'avatar', 'theme.asset-kind-mismatch');
  if (fonts) {
    assertUnique(fonts.faces?.map((face) => face.id) ?? [], `${manifest.id} font face id`, 'theme.font-metadata-invalid');
    assertUnique(fonts.stacks?.map((stack) => stack.id) ?? [], `${manifest.id} font stack id`, 'theme.font-stack-unresolved');
    const faceIds = new Set(fonts.faces.map((face) => face.id));
    for (const face of fonts.faces) {
      if (Object.hasOwn(face, 'runtimeFamily')) throw themeError('theme.package-invalid', `${manifest.id}: ${face.id} runtimeFamily is compiler-owned`);
      const asset = requireAsset(face.assetId, 'font', 'theme.asset-kind-mismatch');
      if (!fontFamilyMatchesMetadata(face.family, asset.fontMetadata.family)) throw themeError('theme.font-metadata-invalid', `${manifest.id}: ${face.id} family ${face.family} does not match ${face.assetId} metadata family ${asset.fontMetadata.family}`);
      if (face.weightMin > face.weightMax) throw themeError('theme.font-metadata-invalid', `${manifest.id}: ${face.id} font weight range is not ordered`);
      if (face.weightMin < asset.fontMetadata.weightMin || face.weightMax > asset.fontMetadata.weightMax) throw themeError('theme.font-metadata-invalid', `${manifest.id}: ${face.id} font weight range is not supported by ${face.assetId}`);
      for (const locale of face.coverage.locales ?? []) try { new Intl.Locale(locale); } catch { throw themeError('theme.font-coverage-invalid', `${manifest.id}: invalid locale ${locale}`); }
    }
    for (const stack of fonts.stacks) for (const faceId of stack.defaultFaces) if (!faceIds.has(faceId)) throw themeError('theme.font-stack-unresolved', `${manifest.id}: ${stack.id} references ${faceId}`);
    const stackIds = new Set(fonts.stacks.map((stack) => stack.id));
    for (const scheme of ['light', 'dark']) for (const id of [presets[scheme].typography.uiStackId, presets[scheme].typography.editorStackId]) if (!stackIds.has(id)) throw themeError('theme.font-stack-unresolved', `${manifest.id}: unknown stack ${id}`);
  }
  if (icons) for (const descriptor of iconDescriptors(icons)) requireAsset(descriptor.assetId, 'icon', 'theme.icon-asset-invalid');
  if (wallpapers) for (const descriptor of wallpaperDescriptors(wallpapers)) requireAsset(descriptor.assetId, 'wallpaper', 'theme.wallpaper-asset-invalid');
}

function iconDescriptors(icons) { return [Object.values(icons.defaults ?? {}), Object.values(icons.schemes?.light ?? {}), Object.values(icons.schemes?.dark ?? {})].flat(); }
function wallpaperDescriptors(wallpapers) { return [Object.values(wallpapers.light ?? {}), Object.values(wallpapers.dark ?? {})].flat(); }
function fontFamilyMatchesMetadata(declaredFamily, metadataFamily) {
  const normalize = (value) => String(value).normalize('NFKC').toLocaleLowerCase()
    .replace(/\b(?:variable(?:\s+font)?|vf)\b/gu, '')
    .replace(/[^\p{Letter}\p{Number}]+/gu, '');
  const declared = normalize(declaredFamily);
  const metadata = normalize(metadataFamily);
  return metadata.length >= 4 && declared.endsWith(metadata);
}
function buildRuntimeFonts(fonts, themeId) {
  return {
    ...fonts,
    faces: fonts.faces.map((face) => ({
      ...face,
      runtimeFamily: themeId === 'builtin.gold-band' ? face.family : `${face.family} [${themeId}]`,
    })),
  };
}

function validateRecipeInvariants(themePackage) {
  for (const role of RECIPE_ROLE_NAMES) {
    const recipe = themePackage.recipes[role];
    for (const state of THEME_VISUAL_STATES) {
      const override = recipe.states?.[state];
      if (override?.background && !override.foreground) throw themeError('theme.recipe-state-invalid', `${themePackage.id}: ${role}.${state} changes background without foreground`);
      if (override?.press && state !== 'active') throw themeError('theme.recipe-state-invalid', `${themePackage.id}: press is only valid for active state`);
      if (override?.press && recipe.motion !== 'press') throw themeError('theme.recipe-state-invalid', `${themePackage.id}: ${role}.${state} requires press motion`);
    }
  }
}

function compilePackageCss(themePackage) {
  const selector = `:root[data-theme='${themePackage.id}']`;
  const blocks = [];
  if (themePackage.fonts) {
    const assets = new Map(themePackage.assets.records.map((asset) => [asset.id, asset]));
    for (const face of themePackage.fonts.faces) {
      const asset = assets.get(face.assetId);
      const format = asset.mediaType === 'font/woff2' ? 'woff2' : 'woff';
      const coverage = face.coverage.unicodeRanges?.length ? `unicode-range:${face.coverage.unicodeRanges.join(',')};` : '';
      const metrics = face.metrics ?? {};
      const weight = face.weightMin === face.weightMax ? `${face.weightMin}` : `${face.weightMin} ${face.weightMax}`;
      blocks.push(`@font-face{font-family:${quoteCss(face.runtimeFamily)};src:url(${quoteCss(asset.outputUrl)}) format('${format}');font-weight:${weight};font-style:${face.style};font-display:swap;${coverage}${fontMetricCss(metrics)}}`);
    }
  }
  for (const [schemeName, scheme] of Object.entries(themePackage.schemes)) {
    const declarations = Object.entries(scheme.semantic).map(([name, value]) => `${semanticCssVariable(name)}:${value}`);
    declarations.push(...tokenDeclarations(scheme), ...typographyDeclarations(themePackage, scheme));
    blocks.push(`${selector}[data-color-scheme='${schemeName}']{${declarations.join(';')}}`);
  }
  blocks.push(
    `${selector} body,${selector} #root,${selector} .app-window-shell{background-color:var(--gold-workspace);background-image:var(--gb-theme-background-image);background-attachment:fixed;background-size:cover}`,
    `${selector} [data-theme-wallpaper-slot]{position:relative;isolation:isolate}`,
    `${selector} [data-theme-wallpaper-slot]::before,${selector} [data-theme-wallpaper-slot]::after{position:absolute;inset:0;pointer-events:none;content:""}`,
    `${selector} [data-theme-wallpaper-slot]::before{z-index:-2;background-image:var(--gb-wallpaper-image,none);background-position:var(--gb-wallpaper-position,center);background-size:var(--gb-wallpaper-size,cover);background-repeat:var(--gb-wallpaper-repeat,no-repeat);opacity:var(--gb-wallpaper-opacity,1)}`,
    `${selector} [data-theme-wallpaper-slot]::after{z-index:-1;background:color-mix(in srgb,var(--gb-wallpaper-overlay-color,transparent) calc(var(--gb-wallpaper-overlay-opacity,0)*100%),transparent)}`,
  );
  const recipeComponentBlocks = [];
  for (const [role, recipe] of Object.entries(themePackage.recipes)) {
    recipeComponentBlocks.push(compileRecipeCss(selector, role, recipe));
    recipeComponentBlocks.push(compileRecipeGeometryCss(selector, role, recipe));
  }
  blocks.push(`@layer components{${recipeComponentBlocks.join('\n')}}`);
  if (themePackage.visualQualityProfiles) {
    const performance = themePackage.visualQualityProfiles.performance;
    blocks.push(`${selector}[data-visual-quality='performance']{--gb-material-blur:${performance.blur}px;--gb-material-saturate:${performance.saturate}%;--gb-theme-texture-opacity:${performance.textureOpacity}${performance.wallpapers ? ';--gb-wallpaper-image:none' : ''}}`);
  }
  blocks.push(`${selector}{--gb-theme-package-version:'${themePackage.version}'}`);
  blocks.push(`@media (prefers-reduced-motion:reduce){${selector} [data-theme-role]{transition-duration:.01ms!important;animation-duration:.01ms!important}}`);
  return blocks.join('\n');
}

function tokenDeclarations(scheme) {
  return [
    `--gb-material-model:${scheme.material.model}`, `--gb-material-opacity:${scheme.material.surfaceOpacity}`,
    `--gb-material-border-highlight:${scheme.material.borderHighlight}`, `--gb-material-surface-overlay:${scheme.material.surfaceOverlay}`,
    `--gb-material-blur:${scheme.material.blur}px`, `--gb-material-saturate:${scheme.material.saturate}%`,
    `--gb-material-backdrop-brightness:${scheme.material.backdropBrightness}%`, `--gb-material-backdrop-contrast:${scheme.material.backdropContrast}%`,
    `--gb-material-specular-highlight:${scheme.material.specularHighlight}`, `--gb-material-edge-shadow:${scheme.material.edgeShadow}`,
    `--gb-theme-background-image:${scheme.material.backgroundImage}`, `--gb-theme-texture-opacity:${scheme.material.textureOpacity}`,
    `--radius:${scheme.shape.radiusControl}`, `--gb-radius-control:${scheme.shape.radiusControl}`, `--gb-radius-surface:${scheme.shape.radiusSurface}`,
    `--gb-radius-overlay:${scheme.shape.radiusOverlay}`, `--gb-radius-avatar:${scheme.shape.radiusAvatar}`, `--gb-radius-pill:${scheme.shape.radiusPill}`,
    `--gb-border-hairline:${scheme.shape.borderHairline}`, `--gb-border-default:${scheme.shape.borderDefault}`, `--gb-border-strong:${scheme.shape.borderStrong}`,
    `--gb-elevation-none:${scheme.elevation.none}`, `--gb-elevation-surface:${scheme.elevation.surface}`, `--gb-elevation-overlay:${scheme.elevation.overlay}`,
    `--gb-elevation-floating:${scheme.elevation.floating}`, `--gb-elevation-pressed:${scheme.elevation.pressed}`, `--gb-press-offset:${scheme.elevation.pressOffset}px`,
    `--gb-motion-mode:${scheme.motion.mode}`, `--gb-motion-fast:${scheme.motion.durationFast}`, `--gb-motion-normal:${scheme.motion.durationNormal}`, `--gb-motion-slow:${scheme.motion.durationSlow}`,
    `--gb-easing-standard:${scheme.motion.easingStandard}`, `--gb-easing-enter:${scheme.motion.easingEnter}`, `--gb-easing-press:${scheme.motion.easingPress}`,
    `--gb-scrollbar-width:${scheme.scrollbar.width}`, `--gb-scrollbar-thumb-radius:${scheme.scrollbar.thumbRadius}`, `--gb-scrollbar-thumb-inset:${scheme.scrollbar.thumbInset}`, `--gb-scrollbar-min-length:${scheme.scrollbar.minLength}`,
  ];
}

function typographyDeclarations(themePackage, scheme) {
  const ui = resolveFontStack(themePackage, scheme.typography.uiStackId);
  const editor = resolveFontStack(themePackage, scheme.typography.editorStackId);
  return [
    `--gb-theme-ui-font-family:${serializeFontFamilies(ui.families, 'sans-serif')}`, `--gb-theme-editor-font-family:${serializeFontFamilies(editor.families, 'monospace')}`,
    `--gb-theme-ui-font-size:${scheme.typography.uiSize}px`, `--gb-theme-editor-font-size:${scheme.typography.editorSize}px`,
    `--gb-theme-ui-line-height:${scheme.typography.uiLineHeight}`, `--gb-theme-editor-line-height:${scheme.typography.editorLineHeight}`,
    `--gb-font-weight-read:${scheme.typography.weights.read}`, `--gb-font-weight-emphasize:${scheme.typography.weights.emphasize}`, `--gb-font-weight-announce:${scheme.typography.weights.announce}`,
  ];
}

function resolveFontStack(themePackage, stackId) {
  const safe = stackId.includes('editor') ? { families: ['JetBrains Mono', 'SFMono-Regular', 'Consolas'] } : { families: ['Inter Variable', 'Gold Band MiSans', 'Microsoft YaHei UI', 'PingFang SC'] };
  if (!themePackage.fonts) return safe;
  const stack = themePackage.fonts.stacks.find((candidate) => candidate.id === stackId);
  if (!stack) return safe;
  const faces = new Map(themePackage.fonts.faces.map((face) => [face.id, face.runtimeFamily]));
  return { families: [...new Set([...stack.defaultFaces.map((id) => faces.get(id)).filter(Boolean), ...stack.systemFallbacks])] };
}

function compileRecipeCss(selector, role, recipe) {
  const base = recipeDeclarations(recipe);
  const blocks = [`${selector} [data-theme-role='${role}']{${base.join(';')}}`];
  if (recipe.material !== 'flat') {
    const backdrop = materialBackdropDeclaration();
    blocks.push(`${selector} [data-theme-role='${role}']:not([data-theme-material-layer='isolated']){${backdrop}}`);
    blocks.push(`${selector} [data-theme-role='${role}'][data-theme-material-layer='isolated']::before{position:absolute;inset:0;pointer-events:none;content:"";border-radius:inherit;${backdrop}}`);
  }
  for (const [state, override] of Object.entries(recipe.states ?? {})) {
    const pseudo = state === 'selected' ? `[data-selected='true']` : state === 'focus' ? ':focus-visible' : state === 'disabled' ? ':disabled' : `:${state}`;
    blocks.push(`${selector} [data-theme-role='${role}']${pseudo}{${stateDeclarations(override).join(';')}}`);
  }
  return blocks.join('\n');
}

function compileRecipeGeometryCss(selector, role, recipe) {
  return `${selector} [data-theme-role='${role}']{border-width:${borderWidthVariable(recipe.borderWidth)};border-style:${recipe.borderStyle};border-radius:${radiusVariable(recipe.radius)}}`;
}

function recipeDeclarations(recipe) {
  const declarations = [
    `--gb-recipe-background:${backgroundVariable(recipe.background)}`, `--gb-recipe-foreground:${foregroundVariable(recipe.foreground)}`, `--gb-recipe-border:${borderVariable(recipe.border)}`,
    `background-color:var(--gb-recipe-background)`, `color:var(--gb-recipe-foreground)`, `border-color:var(--gb-recipe-border)`,
    `box-shadow:${elevationVariable(recipe.elevation)}`,
  ];
  if (recipe.material === 'elevated') declarations.push('background-image:var(--gb-material-surface-overlay)');
  const transitionProperties = motionTransitionProperties(recipe.motion);
  if (transitionProperties) declarations.push(`transition-property:${transitionProperties}`, 'transition-duration:var(--gb-motion-fast)', 'transition-timing-function:var(--gb-easing-standard)');
  return declarations;
}

function materialBackdropDeclaration() {
  return 'backdrop-filter:blur(var(--gb-material-blur)) saturate(var(--gb-material-saturate)) brightness(var(--gb-material-backdrop-brightness)) contrast(var(--gb-material-backdrop-contrast))';
}

function motionTransitionProperties(motion) {
  if (motion === 'none') return '';
  if (motion === 'color') return 'color,background-color,border-color';
  if (motion === 'surface') return 'color,background-color,border-color,box-shadow';
  return 'color,background-color,border-color,box-shadow,transform';
}

function stateDeclarations(state) {
  const declarations = [];
  if (state.background) declarations.push(`background-color:${backgroundVariable(state.background)}`);
  if (state.foreground) declarations.push(`color:${foregroundVariable(state.foreground)}`);
  if (state.border) declarations.push(`border-color:${borderVariable(state.border)}`);
  if (state.elevation) declarations.push(`box-shadow:${elevationVariable(state.elevation)}`);
  if (state.opacity !== undefined) declarations.push(`opacity:${state.opacity}`);
  if (state.press) declarations.push('transform:translateY(var(--gb-press-offset))');
  return declarations;
}

function fontMetricCss(metrics) { return Object.entries({ sizeAdjust: 'size-adjust', ascentOverride: 'ascent-override', descentOverride: 'descent-override', lineGapOverride: 'line-gap-override' }).filter(([key]) => metrics[key]).map(([key, css]) => `${css}:${metrics[key]};`).join(''); }
function serializeFontFamilies(families, fallback) { return [...families.map(quoteCss), fallback].join(', '); }
function quoteCss(value) { return `"${String(value).replaceAll('\\', '\\\\').replaceAll('"', '\\"')}"`; }
function backgroundVariable(value) { if (value === 'transparent') return 'transparent'; return `var(${({ background: '--background', card: '--card', popover: '--popover', sidebar: '--sidebar', 'surface-low': '--gold-surface-low', 'surface-high': '--gold-surface-high', accent: '--accent', primary: '--primary', 'message-user': '--message-user', 'message-assistant': '--gb-message-assistant', composer: '--gb-composer', activity: '--gb-activity', 'tool-card': '--gb-tool-card', 'permission-card': '--gb-permission-card', 'workspace-tab': '--gb-workspace-tab', editor: '--gb-editor' })[value]})`; }
function foregroundVariable(value) { return `var(${({ foreground: '--foreground', 'muted-foreground': '--muted-foreground', 'card-foreground': '--card-foreground', 'accent-foreground': '--accent-foreground', 'primary-foreground': '--primary-foreground', 'message-user-foreground': '--message-user-foreground', 'message-assistant-foreground': '--gb-message-assistant-foreground', 'composer-foreground': '--gb-composer-foreground', 'activity-foreground': '--gb-activity-foreground', 'tool-card-foreground': '--gb-tool-card-foreground', 'permission-card-foreground': '--gb-permission-card-foreground', 'workspace-tab-foreground': '--gb-workspace-tab-foreground', 'editor-foreground': '--gb-editor-foreground' })[value]})`; }
function borderVariable(value) { if (value === 'transparent') return 'transparent'; return `var(${({ border: '--border', 'sidebar-border': '--sidebar-border', highlight: '--gb-material-border-highlight', ring: '--ring', primary: '--primary' })[value]})`; }
function borderWidthVariable(value) { return value === 'none' ? '0' : `var(--gb-border-${value})`; }
function radiusVariable(value) { return value === 'none' ? '0' : `var(--gb-radius-${value})`; }
function elevationVariable(value) { return `var(--gb-elevation-${value})`; }

function semanticCssVariable(name) {
  const kebab = name.replace(/[A-Z]/gu, (letter) => `-${letter.toLowerCase()}`);
  if (name === 'selection') return '--text-selection';
  if (name === 'selectionForeground') return '--text-selection-foreground';
  if (name === 'messageUser' || name === 'messageUserForeground') return `--${kebab}`;
  if (name.startsWith('sidebar') || name.startsWith('titlebar')) return `--${kebab}`;
  if (name.startsWith('scrollbar')) return `--gold-${kebab}`;
  if (['workspace', 'surfaceLow', 'surfaceHigh', 'lineSoft', 'windowOutline', 'windowEdgeShadow', 'running', 'success', 'warning', 'danger', 'permission'].includes(name)) return `--gold-${kebab}`;
  if (!SEMANTIC_TOKEN_NAMES.includes(name)) throw themeError('theme.package-invalid', `unknown semantic token ${name}`);
  if (['background', 'foreground', 'title', 'card', 'cardForeground', 'popover', 'popoverForeground', 'primary', 'primaryForeground', 'secondary', 'secondaryForeground', 'muted', 'mutedForeground', 'accent', 'accentForeground', 'destructive', 'border', 'input', 'ring', 'link'].includes(name)) return `--${kebab}`;
  return `--gb-${kebab}`;
}

async function readPackageJson(themeDirectory, packageRelativePath) {
  const target = resolve(themeDirectory, packageRelativePath);
  const themeRoot = await realpath(themeDirectory);
  if (!target.startsWith(`${themeRoot}${sep}`)) throw themeError('theme.asset-path-invalid', packageRelativePath);
  return readJson(target);
}
async function replaceDirectory(source, target) {
  await mkdir(dirname(target), { recursive: true });
  const backup = `${target}.previous-${randomUUID()}`;
  if (await pathExists(target)) await rename(target, backup);
  try { await rename(source, target); } catch (error) { if (await pathExists(backup)) await rename(backup, target); throw error; }
  await rm(backup, { recursive: true, force: true });
}
async function publishAssetSnapshot(source, target) {
  const files = new Set();
  const directories = new Set(['']);
  await mkdir(target, { recursive: true });
  const copySnapshotDirectory = async (fromDirectory, toDirectory, relativeDirectory) => {
    for (const entry of await readdir(fromDirectory, { withFileTypes: true })) {
      const from = join(fromDirectory, entry.name);
      const to = join(toDirectory, entry.name);
      const relativePath = join(relativeDirectory, entry.name);
      if (entry.isDirectory()) {
        directories.add(relativePath);
        await mkdir(to, { recursive: true });
        await copySnapshotDirectory(from, to, relativePath);
      } else {
        files.add(relativePath);
        await copyFile(from, to);
      }
    }
  };
  await copySnapshotDirectory(source, target, '');
  return { files, directories };
}
async function pruneAssetSnapshot(target, snapshot) {
  const pruneDirectory = async (directory, relativeDirectory) => {
    for (const entry of await readdir(directory, { withFileTypes: true })) {
      const path = join(directory, entry.name);
      const relativePath = join(relativeDirectory, entry.name);
      if (entry.isDirectory()) {
        await pruneDirectory(path, relativePath);
        if (!snapshot.directories.has(relativePath)) await rm(path, { recursive: true, force: true });
      } else if (!snapshot.files.has(relativePath)) {
        await rm(path, { force: true });
      }
    }
  };
  await pruneDirectory(target, '');
}
async function replaceFile(source, target) {
  await mkdir(dirname(target), { recursive: true });
  const staged = `${target}.next-${randomUUID()}`;
  await copyFile(source, staged);
  await rename(staged, target);
}
function assertUnique(values, label, code = 'theme.package-invalid') { if (new Set(values).size !== values.length) throw themeError(code, `duplicate ${label}`); }
function assertSchema(validate, value, label, code) { if (validate(value)) return; const details = validate.errors?.map((error) => `${error.instancePath || '/'} ${error.message}`).join('; '); throw themeError(code, `${label}: ${details}`); }
function themeError(code, detail) { return Object.assign(new Error(`${code}: ${detail}`), { code, detail }); }
async function pathExists(path) { try { await stat(path); return true; } catch (error) { if (error.code === 'ENOENT') return false; throw error; } }
async function readJson(path) { return JSON.parse(await readFile(path, 'utf8')); }
async function writeJson(path, value) { await writeFile(path, `${JSON.stringify(value, null, 2)}\n`, 'utf8'); }
