type Environment = Record<string, string | boolean | undefined>;
const invalid = (field: string): never => { throw { code: 'site.config-invalid', params: { field } }; };
function address(value: string, field: string, production: boolean) {
  if (!value || value.includes('\\') || value.startsWith('//') || value.trim() !== value) return invalid(field);
  let url: URL;
  try { url = new URL(value, 'https://deployment.invalid'); } catch { return invalid(field); }
  if (!value.startsWith('/') && !/^https?:\/\//.test(value)) return invalid(field);
  if (url.username || url.password) return invalid(field);
  const host = url.hostname.replace(/^\[|\]$/g, '').replace(/\.$/, '').toLowerCase();
  if (production && (url.protocol !== 'https:' || host === 'localhost' || host.endsWith('.localhost') || host === '::1' || host === '::' || host === '0.0.0.0' || host.startsWith('127.') || /^::ffff:7f[0-9a-f]{2}:/.test(host))) return invalid(field);
  return value;
}
export function siteConfig(env: Environment, production: boolean) {
  const base = String(env.VITE_SITE_BASE || '/');
  if (!base.startsWith('/') || !base.endsWith('/') || base.startsWith('//') || base.includes('\\') || base.includes('?') || base.includes('#') || new URL(base, 'https://deployment.invalid').pathname !== base) return invalid('VITE_SITE_BASE');
  const demoUrl = address(String(env.VITE_DEMO_URL || (production ? '' : 'http://127.0.0.1:1450/')), 'VITE_DEMO_URL', production);
  const downloadUrl = address(String(env.VITE_DOWNLOAD_URL || 'https://github.com/diodeme/Gold-Band/releases'), 'VITE_DOWNLOAD_URL', production);
  return { base, demoUrl, downloadUrl };
}
export function demoHref(value: string, language: string, origin: string) {
  const url = new URL(value, origin); url.searchParams.set('language', language === 'zh' ? 'zh-cn' : 'en');
  return url.href;
}
