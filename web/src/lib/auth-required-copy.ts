export const AUTH_REQUIRED_ERROR_CODE = 'provider.auth-required';

type Translate = (key: string, options?: Record<string, unknown>) => string;

function stringArrayParam(value: unknown): string[] {
  if (!Array.isArray(value)) return [];
  return [...new Set(value
    .filter((item): item is string => typeof item === 'string')
    .map((item) => item.trim())
    .filter(Boolean))];
}

/**
 * Builds the user-facing authentication explanation from structured ACP
 * parameters. Provider diagnostics remain available separately as raw detail.
 */
export function authRequiredCopy(
  t: Translate,
  params: Record<string, unknown> | null | undefined,
): string | null {
  const generic = t('errors.provider.auth-required', { defaultValue: '' });
  if (!generic) return null;
  const missingEnv = stringArrayParam(params?.missingEnv);
  if (missingEnv.length > 0) {
    return t('errors.provider.auth-required-missing-env', {
      env: missingEnv.join(', '),
      defaultValue: generic,
    });
  }
  if (stringArrayParam(params?.methods).length > 0) {
    return t('errors.provider.auth-required-login', { defaultValue: generic });
  }
  return generic;
}
