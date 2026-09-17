import type { AppErrorVm, ManagedAgentDiagnosticVm } from '@/types';

export type AgentDiagnosticTranslate = (
  key: string,
  options?: Record<string, unknown>,
) => string;

const AGENT_DIAGNOSTIC_RAW_MAX_CHARS = 2000;
const AGENT_DIAGNOSTIC_BANNER_REASON_MAX_CHARS = 120;

export function agentDiagnosticError(diagnostic?: ManagedAgentDiagnosticVm | null): AppErrorVm | null {
  const error = diagnostic?.error;
  if (!error?.code) return null;
  return {
    code: error.code,
    params: diagnosticErrorParams(error.params),
    raw: error.raw,
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

export function agentDiagnosticRawReason(diagnostic?: ManagedAgentDiagnosticVm | null): string | null {
  const error = agentDiagnosticError(diagnostic);
  if (!error) return null;
  const parts: string[] = [];
  for (const part of [formatDiagnosticRaw(error.raw), stringParam(error.params.reason), stringParam(error.params.osError)]) {
    if (!part) continue;
    if (parts.some((existing) => existing.includes(part) || part.includes(existing))) continue;
    parts.push(part);
  }
  if (parts.length === 0) return null;
  const text = parts.join('\n\n');
  return text.length > AGENT_DIAGNOSTIC_RAW_MAX_CHARS ? text.slice(0, AGENT_DIAGNOSTIC_RAW_MAX_CHARS) : text;
}

export function agentDiagnosticHelpReason(
  t: AgentDiagnosticTranslate,
  diagnostic?: ManagedAgentDiagnosticVm | null,
): string {
  return agentDiagnosticRawReason(diagnostic) ?? agentDiagnosticMessage(t, diagnostic);
}

export function agentDiagnosticShortReason(
  t: AgentDiagnosticTranslate,
  diagnostic?: ManagedAgentDiagnosticVm | null,
  fallback?: string,
): string {
  return agentDiagnosticBannerReason(t, diagnostic) ?? fallback ?? t('agentManagement.diagnosticFailedFallback');
}

export function agentDiagnosticBannerReason(
  t: AgentDiagnosticTranslate,
  diagnostic?: ManagedAgentDiagnosticVm | null,
  fallback?: string,
): string | null {
  const source = stringParam(fallback) || agentDiagnosticRawReason(diagnostic) || (diagnostic ? agentDiagnosticMessage(t, diagnostic) : '');
  const line = source.split(/\r?\n/, 1)[0]?.trim() ?? '';
  if (!line) return null;
  if (line.length <= AGENT_DIAGNOSTIC_BANNER_REASON_MAX_CHARS) return line;
  return `${line.slice(0, AGENT_DIAGNOSTIC_BANNER_REASON_MAX_CHARS - 1)}…`;
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

function formatDiagnosticRaw(raw: unknown): string | null {
  if (typeof raw === 'string') {
    const text = raw.trim();
    return text || null;
  }
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) {
    return raw == null ? null : jsonText(raw);
  }
  const record = raw as Record<string, unknown>;
  const message = stringParam(record.message);
  if (message) return message;
  const nested = record.data && typeof record.data === 'object' && !Array.isArray(record.data)
    ? stringParam((record.data as Record<string, unknown>).message)
    : '';
  return nested || jsonText(raw);
}

function jsonText(value: unknown): string | null {
  try {
    const text = JSON.stringify(value);
    return text && text !== '{}' && text !== 'null' ? text : null;
  } catch {
    return null;
  }
}

function stringParam(value: unknown): string {
  return typeof value === 'string' ? value.trim() : '';
}
