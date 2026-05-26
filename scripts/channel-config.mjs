import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

export const repoRoot = dirname(dirname(fileURLToPath(import.meta.url)));

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

export function tauriConfigOverlay(config, version) {
  const overlay = {
    productName: config.productName,
    identifier: config.identifier,
    app: {
      windows: [
        {
          title: config.windowTitle,
          width: 1280,
          height: 800,
          minWidth: 1040,
          minHeight: 680,
        },
      ],
      security: {
        csp: null,
      },
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

  if (config.bundleTargets) {
    overlay.bundle = { targets: config.bundleTargets };
  }

  return overlay;
}

export function writeTauriConfigOverlay(config, outputPath, version) {
  mkdirSync(dirname(outputPath), { recursive: true });
  writeFileSync(outputPath, `${JSON.stringify(tauriConfigOverlay(config, version), null, 2)}\n`);
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
