import { appendFileSync } from 'node:fs';
import { randomUUID } from 'node:crypto';

const sourceVariables = {
  APPLE_CERTIFICATE: 'MACOS_APPLE_CERTIFICATE',
  APPLE_CERTIFICATE_PASSWORD: 'MACOS_APPLE_CERTIFICATE_PASSWORD',
  APPLE_SIGNING_IDENTITY: 'MACOS_APPLE_SIGNING_IDENTITY',
  APPLE_ID: 'MACOS_APPLE_ID',
  APPLE_PASSWORD: 'MACOS_APPLE_PASSWORD',
  APPLE_TEAM_ID: 'MACOS_APPLE_TEAM_ID',
};

const githubEnv = process.env.GITHUB_ENV;
if (!githubEnv) {
  throw new Error('GITHUB_ENV is required');
}

const configured = Object.entries(sourceVariables)
  .filter(([, source]) => Boolean(process.env[source]))
  .map(([target]) => target);

if (configured.length === 0) {
  appendFileSync(githubEnv, 'APPLE_SIGNING_IDENTITY=-\n', 'utf8');
  process.exit(0);
}

const missing = Object.entries(sourceVariables)
  .filter(([, source]) => !process.env[source])
  .map(([target]) => target);
if (missing.length > 0) {
  throw new Error(`Incomplete Apple credentials; missing: ${missing.join(', ')}`);
}

for (const [target, source] of Object.entries(sourceVariables)) {
  const delimiter = `GOLD_BAND_${randomUUID().replaceAll('-', '')}`;
  appendFileSync(
    githubEnv,
    `${target}<<${delimiter}\n${process.env[source]}\n${delimiter}\n`,
    'utf8',
  );
}
