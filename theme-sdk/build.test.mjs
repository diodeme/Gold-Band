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

test('builds every declarative package and discovers a package without application changes', async () => {
  const result = await withBuildFixture();

  assert.equal(result.status, 0, result.stderr || result.stdout);
  assert.match(result.stdout, /Built 4 declarative theme packages/u);
  assert.match(result.stdout, /builtin\.neo-brutalist/u);
});

test('rejects a package with a missing required semantic token', async () => {
  const result = await withBuildFixture(async (directory) => {
    const path = join(directory, 'themes', 'glass', 'tokens', 'light.tokens.json');
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
    const path = join(directory, 'themes', 'glass', 'visual-quality', 'performance.json');
    await updateJson(path, (quality) => {
      quality.uiSize = 12;
    });
  });

  assert.notEqual(result.status, 0);
  assert.match(`${result.stderr}\n${result.stdout}`, /additional properties|uiSize/u);
});

test('rejects an unknown material model', async () => {
  const result = await withBuildFixture(async (directory) => {
    const path = join(directory, 'themes', 'glass', 'tokens', 'light.tokens.json');
    await updateJson(path, (tokens) => {
      tokens.material.model.$value = 'plasma';
    });
  });

  assert.notEqual(result.status, 0);
  assert.match(`${result.stderr}\n${result.stdout}`, /model|allowed values|liquid/u);
});
