// Loaded only by the version probes in an isolated temporary directory.
// Capture the runtime actually chosen by npm's launcher (including Windows shims).
const fs = require('node:fs');
const path = require('node:path');
const { createRequire } = require('node:module');

const observation = { nodeVersion: process.versions.node, nodePath: fs.realpathSync(process.execPath) };
if (process.argv[1]) {
  observation.scriptPath = fs.realpathSync(process.argv[1]);
  let directory = path.dirname(observation.scriptPath);
  for (let depth = 0; depth < 6; depth++) {
    const manifest = path.join(directory, 'package.json');
    if (fs.existsSync(manifest)) {
      const pkg = JSON.parse(fs.readFileSync(manifest, 'utf8'));
      if (pkg.name === 'npm') {
        const semver = createRequire(manifest)('semver');
        observation.npm = {
          root: directory,
          version: pkg.version,
          nodeRange: pkg.engines?.node ?? null,
          nodeCompatible: typeof pkg.engines?.node === 'string'
            && semver.validRange(pkg.engines.node) !== null
            && semver.satisfies(process.versions.node, pkg.engines.node),
        };
        break;
      }
    }
    const parent = path.dirname(directory);
    if (parent === directory) break;
    directory = parent;
  }
}
fs.appendFileSync(process.env.GOLD_BAND_RUNTIME_PROBE_OUTPUT, `${JSON.stringify(observation)}\n`, { mode: 0o600 });
