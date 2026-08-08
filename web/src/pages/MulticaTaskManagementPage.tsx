import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Button } from '@/components/ui/button';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { Page, PageHeader } from '@/components/PageScaffold';
import { MulticaRemoteTaskList } from '@/components/conversation/MulticaRemoteTaskList';

/**
 * 远程任务来源（灵活接入，当前仅 multica）。新增来源 = 向 `REMOTE_TASK_SOURCES` 加一项
 * + 在 body 按 `source` 分支渲染对应来源组件（各来源自带数据/刷新）。数据先于接口：
 * `source` 是渲染分流的唯一键，未来扩展无需改动既有 multica 路径。
 */
const REMOTE_TASK_SOURCES = [
  { value: 'multica', labelKey: 'multica.taskManagement.source.multica' },
] as const;
type RemoteTaskSource = (typeof REMOTE_TASK_SOURCES)[number]['value'];

interface MulticaTaskManagementPageProps {
  /// 直达指定远程/本地 run 的会话页（复用本地侧栏 onSelectRun 同路径）。
  onSelectRun: (projectId: string, taskId: string, runId: string) => void;
  /// 远程任务「点击执行」claim 后进入会话准备页（落 conversation-home，预选最近活跃本地工作区）。
  onPrepareMulticaTask: () => void;
  onBack: () => void;
}

/**
 * 远程任务管理页（会话模式专用整页）。
 *
 * 与「agent 管理 / 上下文管理 / 运行模式管理」并列的导航页。页头「任务来源」下拉按来源切换展示
 *（当前仅 multica，为未来多来源接入保留切换位）；multica 来源内容镜像 `MulticaRemoteTaskList`
 *（工作区分组 + 任务行 + 状态色调徽章 + 失败置顶 + 未连接空状态 + 添加工作区 + 手动刷新）。
 * 远程任务的本地落地工作区延迟到执行时在 composer 下拉选（绑定模型已下沉到任务级），故本页 claim 后
 * 只交接 multica 绑定 + 预填正文，本地工作区由 App 在落 conversation-home 时预选最近活跃（决策 c）。
 *
 * 工作台（旧 UI）不做双胞胎（仅会话模式）。
 */
export function MulticaTaskManagementPage({
  onSelectRun,
  onPrepareMulticaTask,
  onBack,
}: MulticaTaskManagementPageProps) {
  const { t } = useTranslation();
  // 任务来源：当前仅 multica；state 化为未来多来源切换预留（默认 multica）。
  const [source, setSource] = useState<RemoteTaskSource>('multica');

  return (
    <Page flush className="flex flex-col">
      <PageHeader
        title={<span className="text-title">{t('multica.taskManagement.title')}</span>}
        subtitle={t('multica.taskManagement.subtitle')}
        actions={
          <>
            <div className="flex items-center gap-2">
              <span className="text-xs text-muted-foreground">{t('multica.taskManagement.source.label')}</span>
              <Select value={source} onValueChange={(v) => setSource(v as RemoteTaskSource)}>
                <SelectTrigger size="sm" className="w-[140px]">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent align="start">
                  {REMOTE_TASK_SOURCES.map((s) => (
                    <SelectItem key={s.value} value={s.value}>{t(s.labelKey)}</SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            <Button variant="outline" size="sm" onClick={onBack}>{t('common.back')}</Button>
          </>
        }
      />
      <div className="min-h-0 flex-1 overflow-y-auto p-5 xl:p-6">
        {source === 'multica' ? (
          <MulticaRemoteTaskList onSelectRun={onSelectRun} onPrepareMulticaTask={onPrepareMulticaTask} />
        ) : null}
      </div>
    </Page>
  );
}
