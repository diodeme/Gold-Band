import type { ReactNode } from 'react';
import { AppCard } from '@/components/AppCard';
import { Card, CardContent, CardHeader } from '@/components/ui/card';
import { ScrollArea } from '@/components/ui/scroll-area';
import { Tabs, TabsList, TabsTrigger } from '@/components/ui/tabs';

export type EntitySectionTab = 'custom' | 'built-in';

interface EntitySectionProps {
  /** 当前激活的分段 */
  tab: EntitySectionTab;
  onTabChange: (tab: EntitySectionTab) => void;
  /** 自定义分段标题 */
  customLabel: string;
  /** 内置分段标题 */
  builtInLabel: string;
  /** 头部右侧操作区（刷新/添加等） */
  actions?: ReactNode;
  /** 标题下方的工具栏（搜索/筛选） */
  toolbar?: ReactNode;
  /** 错误横幅，非空时展示 */
  error?: ReactNode;
  /** 底部区域（分页等） */
  footer?: ReactNode;
  /** 列表内容（卡片网格 + 空状态由消费方自行渲染） */
  children: ReactNode;
}

/**
 * 通用「自定义 / 内置」两段式列表骨架。
 * 角色管理、MCP 管理、SKILL 管理等列表页共享此骨架，
 * 后续任意一处 UI 优化（头部、搜索栏、滚动区）即可全局同步。
 */
export function EntitySection({
  tab,
  onTabChange,
  customLabel,
  builtInLabel,
  actions,
  toolbar,
  error,
  footer,
  children,
}: EntitySectionProps) {
  return (
    <AppCard className="flex h-full min-h-0 flex-col gap-0 py-0">
      <CardHeader className="border-b px-4 pt-2 pb-1">
        <div className="flex flex-wrap items-center justify-between gap-2">
          <Tabs value={tab} onValueChange={(value) => onTabChange(value as EntitySectionTab)}>
            <TabsList variant="line">
              <TabsTrigger value="custom">{customLabel}</TabsTrigger>
              <TabsTrigger value="built-in">{builtInLabel}</TabsTrigger>
            </TabsList>
          </Tabs>
          {actions ? <div className="flex flex-wrap items-center gap-2">{actions}</div> : null}
        </div>
      </CardHeader>
      <CardContent className="flex min-h-0 flex-1 flex-col p-0">
        {toolbar ? (
          <div className="flex flex-col gap-2 border-b px-4 py-1.5 lg:flex-row lg:items-center">
            {toolbar}
          </div>
        ) : null}
        {error ? (
          <div className="m-4 rounded-lg border border-destructive/30 bg-destructive/10 px-3 py-2 text-sm text-destructive">
            {error}
          </div>
        ) : null}
        <ScrollArea className="min-h-0 flex-1">{children}</ScrollArea>
        {footer}
      </CardContent>
    </AppCard>
  );
}
