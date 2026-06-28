import { useTranslation } from 'react-i18next';
import { Info, Loader2, Pencil, Stethoscope, Trash2, Wrench } from 'lucide-react';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Card } from '@/components/ui/card';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui/tooltip';
import { cn } from '@/lib/utils';
import type { McpServerVm } from '../types';

interface McpHealthState {
  status: string;
  message?: string | null;
}

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
}: McpServerCardProps) {
  const { t } = useTranslation();
  return (
    <Card className={cn('group overflow-hidden border-border/50 transition-shadow hover:shadow-sm', !server.enabled && 'opacity-50')}>
      <div className="flex items-center gap-3 border-b border-border/30 px-4 py-3">
        <span
          className={cn(
            'size-2.5 shrink-0 rounded-full ring-1 ring-offset-1 ring-offset-background',
            isChecking ? 'bg-yellow-400 ring-yellow-400/30 animate-pulse' :
            health?.status === 'healthy' ? 'bg-green-500 ring-green-500/30' :
            health?.status === 'auth_required' ? 'bg-yellow-500 ring-yellow-500/30' :
            health?.status === 'unhealthy' ? 'bg-red-500 ring-red-500/30' :
            'bg-gray-400 ring-gray-400/30',
          )}
          title={health?.message ?? ''}
        />
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2">
            <span className="truncate text-sm font-semibold">{server.name}</span>
            <Badge variant="secondary" className="shrink-0 px-1.5 py-0 text-[10px] font-normal">{server.transport === 'stdio' ? 'Stdio' : server.transport === 'sse' ? 'SSE' : 'HTTP'}</Badge>
            {server.helpMessage && (
              <Popover>
                <PopoverTrigger asChild>
                  <button type="button" className="inline-flex shrink-0 text-muted-foreground hover:text-foreground transition-colors" aria-label="帮助信息">
                    <Info className="size-3.5" />
                  </button>
                </PopoverTrigger>
                <PopoverContent side="top" align="start" className="max-w-72 text-xs leading-relaxed whitespace-pre-wrap">
                  {server.helpMessage}
                </PopoverContent>
              </Popover>
            )}
          </div>
          <p className="truncate font-mono text-[11px] text-muted-foreground">{server.command ?? server.url ?? ''}</p>
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
      <div className="flex items-center justify-between gap-1 px-2 py-1.5">
        {health?.message ? (
          <p className="truncate pl-3 text-[11px] text-muted-foreground">{health.message}</p>
        ) : <span />}
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
              <TooltipContent side="top">工具列表</TooltipContent>
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
