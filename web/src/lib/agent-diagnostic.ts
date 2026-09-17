import type { AppErrorVm, ManagedAgentDiagnosticVm } from '@/types';

export type AgentDiagnosticTranslate = (
  key: string,
  options?: Record<string, unknown>,
) => string;

export function agentDiagnosticError(diagnostic?: ManagedAgentDiagnosticVm | null): AppErrorVm | null {
  const error = diagnostic?.error;
  if (!error?.code) return null;
  return {
    code: error.code,
    params: diagnosticErrorParams(error.params),
  };
}

export function agentDiagnosticMessage(
  t: AgentDiagnosticTranslate,
  diagnostic?: ManagedAgentDiagnosticVm | null,
  fallback?: string,
): string {
  const error = agentDiagnosticError(diagnostic);
  const fallbackText = fallback ?? t('agentManagement.diagnosticFailedFallback');
  if (!error) return fallbackText;
  const params = localizationParams(error.params);
  if (error.code === 'acp.adapter-exited' && params.exitCode != null) {
    return t('errors.acp.adapter-exited-with-code', {
      ...params,
      defaultValue: t('errors.acp.adapter-exited', { ...params, defaultValue: fallbackText }),
    });
  }
  return t(`errors.${error.code}`, { ...params, defaultValue: fallbackText });
}

export function agentDiagnosticDetail(diagnostic?: ManagedAgentDiagnosticVm | null): string | null {
  const reason = agentDiagnosticError(diagnostic)?.params.reason;
  return typeof reason === 'string' && reason.trim() ? reason.trim() : null;
}

function diagnosticErrorParams(params: unknown): Record<string, unknown> {
  if (params && typeof params === 'object' && !Array.isArray(params)) {
    return params as Record<string, unknown>;
  }
  return {};
}

function localizationParams(params: Record<string, unknown>): Record<string, unknown> {
  const { reason: _reason, ...rest } = params;
  return rest;
}
