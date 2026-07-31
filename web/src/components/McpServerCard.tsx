import { useTranslation } from 'react-i18next';
import { Info, Loader2, Pencil, Stethoscope, Trash2, Wrench } from 'lucide-react';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Card } from '@/components/ui/card';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui/tooltip';
import { agentIconClass, agentIconSrc } from '@/lib/agent-icons';
import {
  mcpAgentSupportStatus,
  shouldShowCompatibilityLoading,
  type McpAgentCompatibility,
} from '@/lib/mcp-agent-compatibility';
import { cn } from '@/lib/utils';
import type { McpServerVm } from '../types';

interface McpHealthState {
  status: string;
  message?: string | null;
}

/** 传输 flag 按类型着色（低饱和、适配深色主题） */
const TRANSPORT_BADGE_CLASS: Record<McpServerVm['transport'], string> = {
  stdio: 'border-blue-500/30 bg-blue-500/10 text-blue-600 dark:text-blue-400',
  http: 'border-violet-500/30 bg-violet-500/10 text-violet-600 dark:text-violet-400',
  sse: 'border-amber-500/30 bg-amber-500/10 text-amber-600 dark:text-amber-400',
};

const TRANSPORT_LABEL: Record<McpServerVm['transport'], string> = {
  stdio: 'Stdio',
  http: 'HTTP',
  sse: 'SSE',
};

interface McpServerCardProps {
  server: McpServerVm;
  health?: McpHealthState;
  isChecking: boolean;
  isToolsFetching: boolean;
  onToggle: (enabled: boolean) => void;
  onHealthCheck: () => void;
  onShowTools: () => void;
  /** 仅自定义服务器提供：编辑入口 */
  onEdit?: () => void;
  /** 仅自定义服务器提供：删除入口 */
  onDelete?: () => void;
  /** 已配置 agent 的 MCP 兼容性（展示 agent 图标三态）；为空则不渲染 */
  agentCompatibility?: McpAgentCompatibility[];
  /** 触发单 agent 诊断（点击未知态图标）；不提供则未知态不可点击 */
  onDiagnoseAgent?: (agentType: string) => void;
  /** 正在诊断的 agentType（未知态点击后显示 loading） */
  diagnosingAgentType?: string | null;
  /** agentRegistry 尚未就绪；此时若无兼容性数据，显示「检测中」占位 */
  agentCompatLoading?: boolean;
}

export function McpServerCard({
  server,
  health,
  isChecking,
  isToolsFetching,
  onToggle,
  onHealthCheck,
  onShowTools,
  onEdit,
  onDelete,
  agentCompatibility,
  agentCompatLoading,
  onDiagnoseAgent,
  diagnosingAgentType,
}: McpServerCardProps) {
  const { t } = useTranslation();
  const transportLabel = TRANSPORT_LABEL[server.transport] ?? server.transport;
  return (
    <Card className={cn('group flex h-40 flex-col overflow-hidden border-border/50 py-0 transition-shadow hover:shadow-sm', !server.enabled && 'opacity-50')}>
      {/* ── 上区：名称 / 传输 / 健康点 / 开关 ── */}
      <div className="flex items-center gap-3 px-4 py-3">
        <TooltipProvider delayDuration={300}>
          <Tooltip>
            <TooltipTrigger asChild>
              <span
                className={cn(
                  'size-2.5 shrink-0 rounded-full ring-1 ring-offset-1 ring-offset-background',
                  isChecking ? 'bg-yellow-400 ring-yellow-400/30 animate-pulse' :
                  health?.status === 'healthy' ? 'bg-green-500 ring-green-500/30' :
                  health?.status === 'auth_required' ? 'bg-yellow-500 ring-yellow-500/30' :
                  health?.status === 'unhealthy' ? 'bg-red-500 ring-red-500/30' :
                  'bg-gray-400 ring-gray-400/30',
                )}
              />
            </TooltipTrigger>
            <TooltipContent side="bottom" className="max-w-xs whitespace-pre-wrap text-xs leading-relaxed">
              {health?.message ?? t('contextManagement.mcp.noDiagnostic', '暂无诊断信息')}
            </TooltipContent>
          </Tooltip>
        </TooltipProvider>
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2">
            <span className="truncate text-sm font-semibold">{server.name}</span>
            <Badge variant="outline" className={cn('shrink-0 border px-1.5 py-0 text-[10px] font-normal', TRANSPORT_BADGE_CLASS[server.transport])}>{transportLabel}</Badge>
            {server.helpMessage && (
              <Popover>
                <PopoverTrigger asChild>
                  <button type="button" className="inline-flex shrink-0 text-muted-foreground hover:text-foreground transition-colors" aria-label={t('contextManagement.mcp.helpInfo', '帮助信息')}>
                    <Info className="size-3.5" />
                  </button>
                </PopoverTrigger>
                <PopoverContent side="top" align="start" className="max-w-72 text-xs leading-relaxed whitespace-pre-wrap">
                  {server.helpMessage}
                </PopoverContent>
              </Popover>
            )}
          </div>
          <p className="mt-1 truncate font-mono text-[11px] text-muted-foreground">{server.command ?? server.url ?? ''}</p>
        </div>
        <button
          type="button" role="switch" aria-checked={server.enabled}
          className={cn(
            'relative h-5 w-9 shrink-0 rounded-full border transition-colors',
            server.enabled ? 'border-primary bg-primary' : 'border-border/70 bg-muted-foreground/20',
          )}
          onClick={() => onToggle(!server.enabled)}
        >
          <span className={cn('block size-4 rounded-full bg-background shadow-sm transition-transform', server.enabled && 'translate-x-4')} />
        </button>
      </div>
      {/* ── 下区（footer）：agent 兼容性图标 + 健康消息 / 操作按钮 ── */}
      <div className="mt-auto flex items-center justify-between gap-2 border-t border-border/30 px-2 py-1.5">
        <div className="flex min-w-0 items-center gap-2 px-2">
          {agentCompatibility && agentCompatibility.length > 0 ? (
            <div className="flex shrink-0 items-center gap-0.5">
              {agentCompatibility.map((agent) => {
                const status = mcpAgentSupportStatus(server.transport, agent);
                const isDiagnosing = diagnosingAgentType === agent.agentType;
                const clickable = status === 'unknown' && !!onDiagnoseAgent && !isDiagnosing;
                const showLoading = shouldShowCompatibilityLoading(status, isDiagnosing);
                const tip =
                  status === 'supported'
                    ? t('contextManagement.mcp.agentSupports', { agent: agent.label, transport: transportLabel, defaultValue: '{{agent}} 支持 {{transport}} MCP 传输' })
                    : status === 'unsupported'
                      ? t('contextManagement.mcp.agentNotSupports', { agent: agent.label, transport: transportLabel, defaultValue: '{{agent}} 不支持 {{transport}} MCP 传输' })
                      : status === 'unavailable'
                        ? t('contextManagement.mcp.agentUnavailable', {
                          agent: agent.label,
                          reason: agent.diagnosticReason ?? t('agentManagement.diagnosticFailedFallback'),
                          defaultValue: '{{agent}} 当前不可用：{{reason}}',
                        })
                        : isDiagnosing
                          ? t('contextManagement.mcp.agentChecking', { agent: agent.label, defaultValue: '正在更新 {{agent}} 的 MCP 兼容性' })
                          : t('contextManagement.mcp.agentUnknown', { agent: agent.label, defaultValue: '{{agent}} 尚未声明 MCP 兼容性，点击重新检测' });
                return (
                  <TooltipProvider key={agent.agentType} delayDuration={300}>
                    <Tooltip>
                      <TooltipTrigger asChild>
                        <span className="inline-grid size-6 place-items-center rounded-full hover:bg-muted">
                          <button
                            type="button"
                            className="relative grid size-6 place-items-center rounded-full disabled:pointer-events-none"
                            disabled={!clickable}
                            aria-busy={isDiagnosing}
                            aria-label={tip}
                            onClick={clickable ? () => onDiagnoseAgent?.(agent.agentType) : undefined}
                          >
                            {showLoading ? (
                              <Loader2 className="size-3.5 animate-spin text-muted-foreground/70" />
                            ) : (
                              <span className="relative grid size-5 place-items-center">
                                {status === 'supported' && (
                                  <span className="pointer-events-none absolute left-0 top-0 z-10 size-1.5 rounded-full bg-emerald-500 ring-1 ring-background" />
                                )}
                                {status === 'unavailable' && (
                                  <span className="pointer-events-none absolute left-0 top-0 z-10 size-1.5 rounded-full bg-red-500 ring-1 ring-background" />
                                )}
                                <img
                                  src={agentIconSrc(agent.iconKey)}
                                  alt={agent.label}
                                  className={agentIconClass(
                                    agent.iconKey,
                                    cn(
                                      'relative z-0 size-3.5 transition-opacity',
                                      (status === 'unsupported' || status === 'unavailable') && 'grayscale opacity-35',
                                      status === 'unknown' && 'grayscale opacity-55',
                                      clickable && 'cursor-pointer',
                                    ),
                                  )}
                                />
                              </span>
                            )}
                          </button>
                        </span>
                      </TooltipTrigger>
                      <TooltipContent side="top">{tip}</TooltipContent>
                    </Tooltip>
                  </TooltipProvider>
                );
              })}
            </div>
          ) : agentCompatLoading ? (
            <div className="flex items-center gap-1.5 px-1 text-[11px] text-muted-foreground">
              <Loader2 className="size-3 animate-spin" />
              <span>{t('contextManagement.mcp.compatLoading', '检测 Agent 兼容性…')}</span>
            </div>
          ) : null}
        </div>
        <div className="flex shrink-0 items-center gap-1">
          <TooltipProvider delayDuration={300}>
            <Tooltip>
              <TooltipTrigger asChild>
                <Button size="icon" variant="ghost" className="size-8" disabled={isChecking} onClick={onHealthCheck}>
                  {isChecking ? <Loader2 className="size-3.5 animate-spin" /> : <Stethoscope className="size-3.5" />}
                </Button>
              </TooltipTrigger>
              <TooltipContent side="top">{t('contextManagement.mcp.diagnoseServer', 'MCP 服务诊断')}</TooltipContent>
            </Tooltip>
          </TooltipProvider>
          <TooltipProvider delayDuration={300}>
            <Tooltip>
              <TooltipTrigger asChild>
                <Button size="icon" variant="ghost" className="size-8" disabled={isToolsFetching} onClick={onShowTools}>
                  {isToolsFetching ? <Loader2 className="size-3.5 animate-spin" /> : <Wrench className="size-3.5" />}
                </Button>
              </TooltipTrigger>
              <TooltipContent side="top">{t('contextManagement.mcp.toolsList', '工具列表')}</TooltipContent>
            </Tooltip>
          </TooltipProvider>
          {onEdit && (
            <TooltipProvider delayDuration={300}>
              <Tooltip>
                <TooltipTrigger asChild>
                  <Button size="icon" variant="ghost" className="size-8" onClick={onEdit}>
                    <Pencil className="size-3.5" />
                  </Button>
                </TooltipTrigger>
                <TooltipContent side="top">{t('contextManagement.mcp.editServer', 'Edit')}</TooltipContent>
              </Tooltip>
            </TooltipProvider>
          )}
          {onDelete && (
            <TooltipProvider delayDuration={300}>
              <Tooltip>
                <TooltipTrigger asChild>
                  <Button size="icon" variant="ghost" className="size-8 text-muted-foreground hover:text-destructive" onClick={onDelete}>
                    <Trash2 className="size-3.5" />
                  </Button>
                </TooltipTrigger>
                <TooltipContent side="top">{t('contextManagement.mcp.deleteServer', 'Delete')}</TooltipContent>
              </Tooltip>
            </TooltipProvider>
          )}
        </div>
      </div>
    </Card>
  );
}
