import assert from 'node:assert/strict';
import { cp, mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import test from 'node:test';

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const fixtureRoot = join(repoRoot, '.codex-tmp');

async function withBuildFixture(mutate) {
  await mkdir(fixtureRoot, { recursive: true });
  const directory = await mkdtemp(join(fixtureRoot, 'theme-sdk-build-test-'));
  try {
    await Promise.all([
      cp(join(repoRoot, 'theme-sdk'), join(directory, 'theme-sdk'), { recursive: true }),
      cp(join(repoRoot, 'themes'), join(directory, 'themes'), { recursive: true }),
    ]);
    await mutate?.(directory);
    return spawnSync(process.execPath, [join(directory, 'theme-sdk', 'build.mjs')], {
      cwd: directory,
      encoding: 'utf8',
      timeout: 30_000,
    });
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

test('builds every declarative package and discovers a package without application changes', async () => {
  const result = await withBuildFixture();

  assert.equal(result.status, 0, result.stderr || result.stdout);
  assert.match(result.stdout, /Built 2 declarative theme packages/u);
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
  const css = await readFile(join(repoRoot, 'web', 'src', 'themes', 'generated', 'builtin-themes.css'), 'utf8');
  assert.match(css, /--gb-theme-ui-font-family:"Inter Variable", "Gold Band MiSans"/u);

  const rejected = await withBuildFixture(async (directory) => {
    const path = join(directory, 'themes', 'gold-band', 'presets.json');
    await updateJson(path, (presets) => {
      presets.light.typography.ui.families = ['Segoe UI', 'segoe ui'];
    });
  });
  assert.notEqual(rejected.status, 0);
  assert.match(`${rejected.stderr}\n${rejected.stdout}`, /unique|duplicate|families/u);
});
