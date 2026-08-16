import assert from 'node:assert/strict';
import { cp, mkdir, mkdtemp, readFile, rm, symlink, writeFile } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import test from 'node:test';

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const fixtureRoot = join(repoRoot, '.codex-tmp');

async function withBuildFixture(mutate, inspect) {
  await mkdir(fixtureRoot, { recursive: true });
  const directory = await mkdtemp(join(fixtureRoot, 'theme-sdk-build-test-'));
  try {
    await Promise.all([
      cp(join(repoRoot, 'theme-sdk'), join(directory, 'theme-sdk'), { recursive: true }),
      cp(join(repoRoot, 'themes'), join(directory, 'themes'), { recursive: true }),
    ]);
    await mutate?.(directory);
    const result = spawnSync(process.execPath, [join(directory, 'theme-sdk', 'build.mjs')], {
      cwd: directory,
      encoding: 'utf8',
      timeout: 30_000,
    });
    let generatedCss = null;
    try {
      generatedCss = await readFile(join(directory, 'web', 'src', 'themes', 'generated', 'builtin-themes.css'), 'utf8');
    } catch {}
    return { ...result, generatedCss, inspection: await inspect?.(directory, result) };
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
}

async function updateJson(path, update) {
  const value = JSON.parse(await readFile(path, 'utf8'));
  update(value);
  await writeFile(path, `${JSON.stringify(value, null, 2)}\n`, 'utf8');
}

async function addVisualQualityProfile(directory) {
  const themeDirectory = join(directory, 'themes', 'gold-band');
  await updateJson(join(themeDirectory, 'manifest.json'), (manifest) => {
    manifest.capabilities.push('visual-quality-profiles');
    manifest.visualQualityProfiles = {
      default: 'full',
      supported: ['full', 'performance'],
      performance: 'visual-quality/performance.json',
    };
  });
  await mkdir(join(themeDirectory, 'visual-quality'), { recursive: true });
  await writeFile(join(themeDirectory, 'visual-quality', 'performance.json'), `${JSON.stringify({
    blur: 0,
    saturate: 100,
    shadow: 'none',
    textureOpacity: 0,
    motionDuration: '0ms',
  }, null, 2)}\n`, 'utf8');
}

const onePixelPng = Buffer.from(
  'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=',
  'base64',
);

function buildOutput(result) {
  return `${result.stderr ?? ''}\n${result.stdout ?? ''}`;
}

async function addRasterAsset(directory, {
  assetId,
  kind,
  fileName = `${assetId}.png`,
  data = onePixelPng,
}) {
  const themeDirectory = join(directory, 'themes', 'gold-band');
  const assetDirectory = join(themeDirectory, 'assets', `${kind}s`);
  await mkdir(assetDirectory, { recursive: true });
  await writeFile(join(assetDirectory, fileName), data);
  await updateJson(join(themeDirectory, 'resources.json'), (resources) => {
    resources.assets.push({
      id: assetId,
      kind,
      path: `assets/${kind}s/${fileName}`,
      licenseId: 'inter-ofl',
      required: true,
    });
  });
}

async function enableIconCapability(directory, descriptor = {
  assetId: 'fixture-icon', renderMode: 'mask', nativeSize: 20, imageRendering: 'auto',
}) {
  const themeDirectory = join(directory, 'themes', 'gold-band');
  await updateJson(join(themeDirectory, 'manifest.json'), (manifest) => manifest.capabilities.push('icons'));
  if (descriptor.assetId === 'fixture-icon') {
    await addRasterAsset(directory, { assetId: 'fixture-icon', kind: 'icon' });
  }
  await writeFile(join(themeDirectory, 'icons.json'), `${JSON.stringify({
    defaults: { 'navigation.search': descriptor },
    schemes: { dark: { 'navigation.search': { ...descriptor, renderMode: 'image' } } },
  }, null, 2)}\n`, 'utf8');
}

async function enableWallpaperCapability(directory, descriptor = {
  assetId: 'fixture-wallpaper', fit: 'cover', position: 'center', repeat: 'no-repeat',
  opacity: 1, overlayColor: 'background', overlayOpacity: 0.5,
}) {
  const themeDirectory = join(directory, 'themes', 'gold-band');
  await updateJson(join(themeDirectory, 'manifest.json'), (manifest) => manifest.capabilities.push('wallpapers'));
  if (descriptor.assetId === 'fixture-wallpaper') {
    await addRasterAsset(directory, { assetId: 'fixture-wallpaper', kind: 'wallpaper' });
  }
  await writeFile(join(themeDirectory, 'wallpapers.json'), `${JSON.stringify({
    light: { app: descriptor },
    dark: { app: descriptor },
  }, null, 2)}\n`, 'utf8');
}

test('builds every declarative package and discovers a package without application changes', async () => {
  const result = await withBuildFixture();

  assert.equal(result.status, 0, result.stderr || result.stdout);
  assert.match(result.stdout, /Built 2 Theme Contract v2 packages/u);
  assert.match(result.stdout, /builtin\.gold-band/u);
  assert.match(result.stdout, /builtin\.tech-neutral/u);
});

test('rejects a package with a missing required semantic token', async () => {
  const result = await withBuildFixture(async (directory) => {
    const path = join(directory, 'themes', 'tech-neutral', 'tokens', 'light.tokens.json');
    await updateJson(path, (tokens) => delete tokens.semantic.editor);
  });

  assert.notEqual(result.status, 0);
  assert.match(`${result.stderr}\n${result.stdout}`, /editor|required property/u);
});

test('rejects circular DTCG aliases', async () => {
  const result = await withBuildFixture(async (directory) => {
    const path = join(directory, 'themes', 'gold-band', 'tokens', 'primitives.tokens.json');
    await updateJson(path, (tokens) => {
      tokens.primitive.radius.$value = '{primitive.motionEasing}';
      tokens.primitive.motionEasing.$value = '{primitive.radius}';
    });
  });

  assert.notEqual(result.status, 0);
  assert.match(`${result.stderr}\n${result.stdout}`, /circular|cycle|reference/u);
});

test('rejects arbitrary recipe fields', async () => {
  const result = await withBuildFixture(async (directory) => {
    const path = join(directory, 'themes', 'tech-neutral', 'recipes.json');
    await updateJson(path, (recipes) => {
      recipes.card.selector = '.business-card';
    });
  });

  assert.notEqual(result.status, 0);
  assert.match(`${result.stderr}\n${result.stdout}`, /additional properties|selector/u);
});

test('rejects visual-quality overrides outside the closed effect whitelist', async () => {
  const result = await withBuildFixture(async (directory) => {
    await addVisualQualityProfile(directory);
    const path = join(directory, 'themes', 'gold-band', 'visual-quality', 'performance.json');
    await updateJson(path, (quality) => {
      quality.uiSize = 12;
    });
  });

  assert.notEqual(result.status, 0);
  assert.match(`${result.stderr}\n${result.stdout}`, /additional properties|uiSize/u);
});

test('rejects an unknown material model', async () => {
  const result = await withBuildFixture(async (directory) => {
    const path = join(directory, 'themes', 'gold-band', 'tokens', 'light.tokens.json');
    await updateJson(path, (tokens) => {
      tokens.material.model.$value = 'plasma';
    });
  });

  assert.notEqual(result.status, 0);
  assert.match(`${result.stderr}\n${result.stdout}`, /model|allowed values|liquid/u);
});

test('builds ordered font stacks and rejects duplicate families', async () => {
  const built = await withBuildFixture();
  assert.equal(built.status, 0, built.stderr || built.stdout);
  assert.match(built.generatedCss, /--gb-theme-ui-font-family:"Inter Variable", "Microsoft YaHei UI"/u);

  const rejected = await withBuildFixture(async (directory) => {
    const path = join(directory, 'themes', 'gold-band', 'fonts.json');
    await updateJson(path, (fonts) => {
      fonts.stacks[0].systemFallbacks = ['Segoe UI', 'Segoe UI'];
    });
  });
  assert.notEqual(rejected.status, 0);
  assert.match(`${rejected.stderr}\n${rejected.stdout}`, /unique|duplicate|families/u);
});

test('emits a complete v2 runtime package with non-distributed icon and wallpaper fixtures', async () => {
  const result = await withBuildFixture(async (directory) => {
    await enableIconCapability(directory);
    await enableWallpaperCapability(directory);
  }, async (directory) => {
    const runtime = JSON.parse(await readFile(join(directory, 'themes', 'gold-band', 'dist', 'runtime-theme.json'), 'utf8'));
    const records = new Map(runtime.assets.records.map((record) => [record.id, record]));
    const icon = records.get('fixture-icon');
    const wallpaper = records.get('fixture-wallpaper');
    return {
      schemaVersion: runtime.schemaVersion,
      contractVersion: runtime.contractVersion,
      capabilities: runtime.capabilities,
      iconKind: icon?.kind,
      wallpaperKind: wallpaper?.kind,
      iconExists: Boolean(icon && await readFile(join(directory, 'web', 'public', icon.outputUrl.slice(1)))),
      wallpaperExists: Boolean(wallpaper && await readFile(join(directory, 'web', 'public', wallpaper.outputUrl.slice(1)))),
    };
  });

  assert.equal(result.status, 0, buildOutput(result));
  assert.deepEqual(result.inspection, {
    schemaVersion: 2,
    contractVersion: 2,
    capabilities: ['tokens', 'component-recipes', 'fonts', 'avatars', 'icons', 'wallpapers'],
    iconKind: 'icon',
    wallpaperKind: 'wallpaper',
    iconExists: true,
    wallpaperExists: true,
  });
});

test('rejects v1 manifests instead of maintaining a compatibility parser', async () => {
  const result = await withBuildFixture(async (directory) => {
    await updateJson(join(directory, 'themes', 'gold-band', 'manifest.json'), (manifest) => {
      manifest.schemaVersion = 1;
      manifest.contractVersion = 1;
    });
  });
  assert.notEqual(result.status, 0);
  assert.match(buildOutput(result), /theme\.package-invalid|schemaVersion|contractVersion/u);
});

test('enforces capability and declaration-file symmetry in both directions', async (t) => {
  await t.test('rejects an undeclared icons file', async () => {
    const result = await withBuildFixture(async (directory) => {
      await writeFile(join(directory, 'themes', 'gold-band', 'icons.json'), '{"defaults":{}}\n', 'utf8');
    });
    assert.notEqual(result.status, 0);
    assert.match(buildOutput(result), /theme\.capability-file-mismatch/u);
  });
  await t.test('rejects a declared capability without its file', async () => {
    const result = await withBuildFixture(async (directory) => {
      await updateJson(join(directory, 'themes', 'gold-band', 'manifest.json'), (manifest) => manifest.capabilities.push('icons'));
    });
    assert.notEqual(result.status, 0);
    assert.match(buildOutput(result), /theme\.capability-file-mismatch/u);
  });
});

test('rejects missing token aliases and unknown recipe states', async (t) => {
  await t.test('missing alias', async () => {
    const result = await withBuildFixture(async (directory) => {
      const path = join(directory, 'themes', 'gold-band', 'tokens', 'primitives.tokens.json');
      await updateJson(path, (tokens) => { tokens.primitive.radius.$value = '{primitive.doesNotExist}'; });
    });
    assert.notEqual(result.status, 0);
    assert.match(buildOutput(result), /reference|doesNotExist|not found/u);
  });
  await t.test('unknown state', async () => {
    const result = await withBuildFixture(async (directory) => {
      const path = join(directory, 'themes', 'gold-band', 'recipes.json');
      await updateJson(path, (recipes) => { recipes.card.states = { loading: { opacity: 0.5 } }; });
    });
    assert.notEqual(result.status, 0);
    assert.match(buildOutput(result), /theme\.package-invalid|additional properties|loading/u);
  });
});

test('enforces recipe state foreground pairing and press semantics', async (t) => {
  await t.test('background requires foreground', async () => {
    const result = await withBuildFixture(async (directory) => {
      const path = join(directory, 'themes', 'gold-band', 'recipes.json');
      await updateJson(path, (recipes) => { recipes.card.states = { hover: { background: 'accent' } }; });
    });
    assert.notEqual(result.status, 0);
    assert.match(buildOutput(result), /theme\.recipe-state-invalid/u);
  });
  await t.test('press is active-only', async () => {
    const result = await withBuildFixture(async (directory) => {
      const path = join(directory, 'themes', 'gold-band', 'recipes.json');
      await updateJson(path, (recipes) => { recipes.card.states = { hover: { press: true } }; });
    });
    assert.notEqual(result.status, 0);
    assert.match(buildOutput(result), /theme\.recipe-state-invalid/u);
  });
});

test('rejects escaped and symbolic-link asset paths', async (t) => {
  await t.test('normalized traversal', async () => {
    const result = await withBuildFixture(async (directory) => {
      const path = join(directory, 'themes', 'gold-band', 'resources.json');
      await updateJson(path, (resources) => { resources.assets[0].path = 'assets/fonts/../fonts/inter-latin-variable.woff2'; });
    });
    assert.notEqual(result.status, 0);
    assert.match(buildOutput(result), /theme\.asset-path-invalid/u);
  });
  await t.test('directory junction', async () => {
    const result = await withBuildFixture(async (directory) => {
      const themeDirectory = join(directory, 'themes', 'gold-band');
      await symlink(join(themeDirectory, 'assets', 'fonts'), join(themeDirectory, 'assets', 'font-link'), 'junction');
      await updateJson(join(themeDirectory, 'resources.json'), (resources) => {
        resources.assets.push({ id: 'linked-font', kind: 'font', path: 'assets/font-link/inter-latin-variable.woff2', licenseId: 'inter-ofl' });
      });
    });
    assert.notEqual(result.status, 0);
    assert.match(buildOutput(result), /theme\.asset-path-invalid|symbolic links are forbidden/u);
  });
});

test('rejects duplicate IDs and case-insensitive physical-file aliases', async (t) => {
  await t.test('duplicate stable id', async () => {
    const result = await withBuildFixture(async (directory) => {
      await updateJson(join(directory, 'themes', 'gold-band', 'resources.json'), (resources) => {
        resources.assets.push({ ...resources.assets[0] });
      });
    });
    assert.notEqual(result.status, 0);
    assert.match(buildOutput(result), /theme\.asset-id-duplicate/u);
  });
  await t.test('case-insensitive physical alias', async () => {
    const result = await withBuildFixture(async (directory) => {
      await updateJson(join(directory, 'themes', 'gold-band', 'resources.json'), (resources) => {
        resources.assets.push({ id: 'inter-alias', kind: 'font', path: 'assets/fonts/INTER-LATIN-VARIABLE.WOFF2', licenseId: 'inter-ofl' });
      });
    });
    assert.notEqual(result.status, 0);
    assert.match(buildOutput(result), /theme\.asset-id-duplicate|one physical file/u);
  });
});

test('rejects missing licenses, signature mismatches, dimensions, counts, and font byte budgets', async (t) => {
  await t.test('missing license', async () => {
    const result = await withBuildFixture(async (directory) => {
      await updateJson(join(directory, 'themes', 'gold-band', 'resources.json'), (resources) => { resources.assets[0].licenseId = 'missing'; });
    });
    assert.notEqual(result.status, 0);
    assert.match(buildOutput(result), /theme\.asset-license-missing/u);
  });
  await t.test('signature mismatch', async () => {
    const result = await withBuildFixture(async (directory) => {
      await writeFile(join(directory, 'themes', 'gold-band', 'assets', 'fonts', 'inter-latin-variable.woff2'), onePixelPng);
    });
    assert.notEqual(result.status, 0);
    assert.match(buildOutput(result), /theme\.asset-media-type-invalid/u);
  });
  await t.test('icon dimensions', async () => {
    const tooWide = Buffer.from(onePixelPng);
    tooWide.writeUInt32BE(129, 16);
    const result = await withBuildFixture(async (directory) => {
      await enableIconCapability(directory);
      await writeFile(join(directory, 'themes', 'gold-band', 'assets', 'icons', 'fixture-icon.png'), tooWide);
    });
    assert.notEqual(result.status, 0);
    assert.match(buildOutput(result), /theme\.asset-dimensions-exceeded/u);
  });
  await t.test('asset count', async () => {
    const result = await withBuildFixture(async (directory) => {
      await updateJson(join(directory, 'themes', 'gold-band', 'resources.json'), (resources) => {
        resources.assets = Array.from({ length: 129 }, (_, index) => ({ ...resources.assets[0], id: `font-${index}` }));
      });
    });
    assert.notEqual(result.status, 0);
    assert.match(buildOutput(result), /theme\.asset-count-exceeded/u);
  });
  await t.test('font byte budget', async () => {
    const oversized = Buffer.alloc(8 * 1024 * 1024 + 1);
    oversized.write('wOF2', 0, 'ascii');
    const result = await withBuildFixture(async (directory) => {
      await writeFile(join(directory, 'themes', 'gold-band', 'assets', 'fonts', 'inter-latin-variable.woff2'), oversized);
    });
    assert.notEqual(result.status, 0);
    assert.match(buildOutput(result), /theme\.asset-budget-exceeded/u);
  });
});

test('rejects invalid font coverage, unresolved stacks, and metadata mismatches', async (t) => {
  await t.test('invalid locale', async () => {
    const result = await withBuildFixture(async (directory) => {
      await updateJson(join(directory, 'themes', 'gold-band', 'fonts.json'), (fonts) => { fonts.faces[0].coverage.locales = ['not_a_locale']; });
    });
    assert.notEqual(result.status, 0);
    assert.match(buildOutput(result), /theme\.font-coverage-invalid/u);
  });
  await t.test('unresolved face', async () => {
    const result = await withBuildFixture(async (directory) => {
      await updateJson(join(directory, 'themes', 'gold-band', 'fonts.json'), (fonts) => { fonts.stacks[0].defaultFaces = ['missing-face']; });
    });
    assert.notEqual(result.status, 0);
    assert.match(buildOutput(result), /theme\.font-stack-unresolved/u);
  });
  await t.test('declared family must match font metadata', async () => {
    const result = await withBuildFixture(async (directory) => {
      await updateJson(join(directory, 'themes', 'gold-band', 'fonts.json'), (fonts) => { fonts.faces[0].family = 'Unrelated Typeface'; });
    });
    assert.notEqual(result.status, 0, 'font family metadata mismatch must be rejected');
    assert.match(buildOutput(result), /theme\.font-metadata-invalid/u);
  });
});

test('validates icon slots, descriptors, and asset kinds', async (t) => {
  await t.test('unknown slot', async () => {
    const result = await withBuildFixture(async (directory) => {
      await enableIconCapability(directory);
      await updateJson(join(directory, 'themes', 'gold-band', 'icons.json'), (icons) => { icons.defaults['navigation.unknown'] = icons.defaults['navigation.search']; });
    });
    assert.notEqual(result.status, 0);
    assert.match(buildOutput(result), /theme\.package-invalid|navigation\.unknown|additional properties/u);
  });
  await t.test('invalid render mode and native size', async () => {
    const result = await withBuildFixture(async (directory) => {
      await enableIconCapability(directory, { assetId: 'fixture-icon', renderMode: 'svg', nativeSize: 18, imageRendering: 'auto' });
    });
    assert.notEqual(result.status, 0);
    assert.match(buildOutput(result), /theme\.package-invalid|renderMode|nativeSize/u);
  });
  await t.test('wrong asset kind', async () => {
    const result = await withBuildFixture(async (directory) => {
      await enableIconCapability(directory, { assetId: 'inter-latin-variable', renderMode: 'mask', nativeSize: 20, imageRendering: 'auto' });
    });
    assert.notEqual(result.status, 0);
    assert.match(buildOutput(result), /theme\.icon-asset-invalid/u);
  });
});

test('validates wallpaper slots, descriptors, and asset kinds', async (t) => {
  await t.test('unknown slot and invalid opacity', async () => {
    const result = await withBuildFixture(async (directory) => {
      await enableWallpaperCapability(directory);
      await updateJson(join(directory, 'themes', 'gold-band', 'wallpapers.json'), (wallpapers) => {
        wallpapers.light.unknown = { ...wallpapers.light.app, opacity: 2 };
      });
    });
    assert.notEqual(result.status, 0);
    assert.match(buildOutput(result), /theme\.package-invalid|additional properties|opacity/u);
  });
  await t.test('invalid fit and position', async () => {
    const result = await withBuildFixture(async (directory) => {
      await enableWallpaperCapability(directory, {
        assetId: 'fixture-wallpaper', fit: 'stretch', position: 'middle', repeat: 'no-repeat', opacity: 1,
        overlayColor: 'background', overlayOpacity: 0.5,
      });
    });
    assert.notEqual(result.status, 0);
    assert.match(buildOutput(result), /theme\.package-invalid|fit|position/u);
  });
  await t.test('wrong asset kind', async () => {
    const result = await withBuildFixture(async (directory) => {
      await enableWallpaperCapability(directory, {
        assetId: 'inter-latin-variable', fit: 'cover', position: 'center', repeat: 'no-repeat', opacity: 1,
        overlayColor: 'background', overlayOpacity: 0.5,
      });
    });
    assert.notEqual(result.status, 0);
    assert.match(buildOutput(result), /theme\.wallpaper-asset-invalid/u);
  });
});

test('does not replace last-known-good generated outputs when validation fails', async () => {
  const marker = 'last-known-good';
  const result = await withBuildFixture(async (directory) => {
    const generated = join(directory, 'web', 'src', 'themes', 'generated');
    await mkdir(generated, { recursive: true });
    await writeFile(join(generated, 'catalog.json'), marker, 'utf8');
    await writeFile(join(directory, 'themes', 'gold-band', 'dist', 'runtime-theme.json'), marker, 'utf8');
    await updateJson(join(directory, 'themes', 'tech-neutral', 'manifest.json'), (manifest) => { manifest.contractVersion = 1; });
  }, async (directory) => ({
    catalog: await readFile(join(directory, 'web', 'src', 'themes', 'generated', 'catalog.json'), 'utf8'),
    runtime: await readFile(join(directory, 'themes', 'gold-band', 'dist', 'runtime-theme.json'), 'utf8'),
  }));
  assert.notEqual(result.status, 0);
  assert.deepEqual(result.inspection, { catalog: marker, runtime: marker });
});

test('removes stale generated assets when rebuilding the generated asset directory', async () => {
  const result = await withBuildFixture(async (directory) => {
    const generatedAssets = join(directory, 'web', 'public', 'theme-assets');
    await mkdir(generatedAssets, { recursive: true });
    await writeFile(join(generatedAssets, 'stale.txt'), 'stale', 'utf8');
  }, async (directory) => {
    try {
      await readFile(join(directory, 'web', 'public', 'theme-assets', 'stale.txt'));
      return true;
    } catch {
      return false;
    }
  });
  assert.equal(result.status, 0, buildOutput(result));
  assert.equal(result.inspection, false, 'stale assets must not leak into later frontendDist builds');
});
