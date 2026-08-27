import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

export const repoRoot = dirname(dirname(fileURLToPath(import.meta.url)));
const baseTauriConfig = JSON.parse(
  readFileSync(join(repoRoot, 'src-tauri', 'tauri.conf.json'), 'utf8'),
);

export function readChannelConfig(channel) {
  const configPath = join(repoRoot, 'configs', 'channels', `${channel}.json`);
  let config;
  try {
    config = JSON.parse(readFileSync(configPath, 'utf8'));
  } catch (error) {
    throw new Error(`Unsupported channel: ${channel}. Expected config file at ${configPath}. ${error instanceof Error ? error.message : String(error)}`);
  }

  if (config.channel !== channel) {
    throw new Error(`Channel config mismatch: expected ${channel}, found ${config.channel}.`);
  }

  return config;
}

export function channelEnvPrefix(channel) {
  return channel.toUpperCase().replace(/[^A-Z0-9]/g, '_');
}

export function tauriConfigOverlay(config, version, buildOptions = {}) {
  const overlay = {
    productName: config.productName,
    identifier: config.identifier,
    app: {
      ...baseTauriConfig.app,
      windows: baseTauriConfig.app.windows.map((windowConfig, index) => index === 0
        ? { ...windowConfig, title: config.windowTitle }
        : { ...windowConfig }),
    },
    plugins: {
      updater: {
        pubkey: config.updaterPublicKey,
        endpoints: [config.updaterEndpoint],
        dangerousInsecureTransportProtocol: Boolean(config.allowHttpUpdater),
        windows: {
          installMode: 'passive',
        },
      },
    },
  };

  if (version) {
    overlay.version = version;
  }

  const bundle = {};
  if (config.bundleTargets) {
    bundle.targets = config.bundleTargets;
  }
  if (buildOptions.createUpdaterArtifacts !== undefined) {
    bundle.createUpdaterArtifacts = buildOptions.createUpdaterArtifacts;
  }
  if (Object.keys(bundle).length > 0) {
    overlay.bundle = bundle;
  }

  return overlay;
}

export function writeTauriConfigOverlay(config, outputPath, version, buildOptions) {
  mkdirSync(dirname(outputPath), { recursive: true });
  writeFileSync(
    outputPath,
    `${JSON.stringify(tauriConfigOverlay(config, version, buildOptions), null, 2)}\n`,
  );
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const channel = process.argv[2] ?? 'default';
  const outputPath = process.argv[3] ?? join(repoRoot, 'src-tauri', 'target', 'channel', `tauri.${channel}.conf.json`);
  const version = process.argv[4] || undefined;
  try {
    writeTauriConfigOverlay(readChannelConfig(channel), outputPath, version);
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error));
    process.exit(1);
  }
}
