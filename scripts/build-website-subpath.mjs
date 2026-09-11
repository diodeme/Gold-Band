import { spawnSync } from 'node:child_process';

const base = process.env.WEBSITE_BASE ?? '/site-by-codex/';
const siteDir = process.env.WEBSITE_SITE_DIR ?? '.codex-temp/site-by-codex-dist';
const demoDir = process.env.WEBSITE_DEMO_DIR ?? '.codex-temp/demo-by-codex-dist';
const environment = { ...process.env, WEBSITE_BASE: base, WEBSITE_SITE_DIR: siteDir, WEBSITE_DEMO_DIR: demoDir };

for (const script of ['site:build', 'demo:build', 'website:compose']) {
  const result = spawnSync('npm', ['run', script], { stdio: 'inherit', env: environment, shell: process.platform === 'win32' });
  if (result.status !== 0) process.exit(result.status ?? 1);
}
console.log(JSON.stringify({ base, siteDir, demoDir }));
