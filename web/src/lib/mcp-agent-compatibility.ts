import type { McpServerVm } from '../types';

export interface McpAgentCompatibility {
  agentType: string;
  label: string;
  iconKey: string;
  diagnosticAvailable?: boolean | null;
  diagnosticReason?: string | null;
  mcpHttpSupported?: boolean | null;
  mcpSseSupported?: boolean | null;
}

export type AgentSupportStatus = 'supported' | 'unsupported' | 'unknown' | 'unavailable';

/**
 * Agent 健康状态优先于 transport capability。不可用不等同于不支持，
 * 只表示当前无法建立 ACP 连接来确认或使用 MCP 能力。
 */
export function mcpAgentSupportStatus(
  transport: McpServerVm['transport'],
  agent: McpAgentCompatibility,
): AgentSupportStatus {
  if (agent.diagnosticAvailable === false) return 'unavailable';
  if (transport === 'stdio') return 'supported';
  const flag = transport === 'http' ? agent.mcpHttpSupported : agent.mcpSseSupported;
  if (flag == null) return 'unknown';
  return flag ? 'supported' : 'unsupported';
}

/** doctor 刷新时保留最后一次已知状态；只有没有旧结果时才显示 loading。 */
export function shouldShowCompatibilityLoading(
  status: AgentSupportStatus,
  isDiagnosing: boolean,
): boolean {
  return isDiagnosing && status === 'unknown';
}
