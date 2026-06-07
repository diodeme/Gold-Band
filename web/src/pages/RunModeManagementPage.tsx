import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Bot, Workflow, Save } from 'lucide-react';
import type { AgentRegistryVm, ConversationRunModeVm, WorkflowTemplateStore } from '../types';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { Separator } from '@/components/ui/separator';

interface RunModeManagementPageProps {
  runMode: ConversationRunModeVm;
  agentRegistry: AgentRegistryVm | null;
  workflowTemplates: WorkflowTemplateStore | null;
  onSave: (mode: ConversationRunModeVm) => void;
  onBack: () => void;
}

export function RunModeManagementPage({
  runMode,
  agentRegistry,
  workflowTemplates,
  onSave,
  onBack,
}: RunModeManagementPageProps) {
  const { t } = useTranslation();
  const [mode, setMode] = useState<'auto' | 'workflow'>(runMode.mode);
  const [agent, setAgent] = useState(runMode.autoConfig?.agentType ?? '');
  const [model, setModel] = useState(runMode.autoConfig?.modelId ?? '');
  const [permissionMode, setPermissionMode] = useState(runMode.autoConfig?.permissionMode ?? '');
  const [globalGoal, setGlobalGoal] = useState(runMode.autoConfig?.globalGoal ?? '');
  const [workflowTemplateId, setWorkflowTemplateId] = useState(runMode.workflowTemplateId ?? '');
  const [saved, setSaved] = useState(false);

  const agents = agentRegistry?.agents.filter((a) => a.supported) ?? [];
  const templates = workflowTemplates?.templates ?? [];

  const handleSave = () => {
    const updated: ConversationRunModeVm = mode === 'auto'
      ? { mode: 'auto', autoConfig: { agentType: agent, modelId: model || undefined, permissionMode: permissionMode || undefined, globalGoal: globalGoal || undefined } }
      : { mode: 'workflow', workflowTemplateId: workflowTemplateId || undefined };
    onSave(updated);
    setSaved(true);
    setTimeout(() => setSaved(false), 2000);
  };

  return (
    <div className="flex h-full flex-col overflow-y-auto">
      <div className="mx-auto w-full max-w-2xl space-y-8 px-8 py-10">
        <div className="flex items-center justify-between">
          <div>
            <h1 className="text-xl font-semibold text-foreground">{t('runMode.title')}</h1>
            <p className="mt-1 text-sm text-muted-foreground">
              {mode === 'auto' ? t('runMode.autoDescription') : t('runMode.workflowSection')}
            </p>
          </div>
          <Button variant="outline" size="sm" onClick={onBack}>Back</Button>
        </div>

        {/* Mode selector */}
        <div className="flex rounded-lg bg-muted p-1 w-fit">
          <button
            type="button"
            className={`rounded-md px-4 py-1.5 text-sm font-medium transition-colors ${mode === 'auto' ? 'bg-background text-foreground shadow-sm' : 'text-muted-foreground hover:text-foreground'}`}
            onClick={() => setMode('auto')}
          >
            {t('runMode.autoSection')}
          </button>
          <button
            type="button"
            className={`rounded-md px-4 py-1.5 text-sm font-medium transition-colors ${mode === 'workflow' ? 'bg-background text-foreground shadow-sm' : 'text-muted-foreground hover:text-foreground'}`}
            onClick={() => setMode('workflow')}
          >
            {t('runMode.workflowSection')}
          </button>
        </div>

        <Separator />

        {mode === 'auto' ? (
          <div className="space-y-5">
            <div className="space-y-2">
              <label className="text-sm font-medium"><Bot className="inline size-3.5 mr-1.5" />{t('runMode.agent')}</label>
              <Select value={agent} onValueChange={setAgent}>
                <SelectTrigger className="h-9">
                  <SelectValue placeholder={t('conversation.home.selectAgent')} />
                </SelectTrigger>
                <SelectContent>
                  {agents.map((a) => (
                    <SelectItem key={a.agentType} value={a.agentType}>{a.displayName}</SelectItem>
                  ))}
                  {agents.length === 0 ? (
                    <div className="px-2 py-3 text-xs text-muted-foreground">{t('runMode.noAgentConfigured')}</div>
                  ) : null}
                </SelectContent>
              </Select>
            </div>

            <div className="space-y-2">
              <label className="text-sm font-medium">{t('runMode.model')}</label>
              <Input className="h-9" value={model} onChange={(e) => setModel(e.target.value)} placeholder="e.g. claude-sonnet-4-6" />
            </div>

            <div className="space-y-2">
              <label className="text-sm font-medium">{t('runMode.permissionMode')}</label>
              <Select value={permissionMode} onValueChange={setPermissionMode}>
                <SelectTrigger className="h-9"><SelectValue placeholder="Default" /></SelectTrigger>
                <SelectContent>
                  <SelectItem value="default">Default</SelectItem>
                  <SelectItem value="accept-edits">Accept Edits</SelectItem>
                  <SelectItem value="bypass-permissions">Bypass Permissions</SelectItem>
                  <SelectItem value="plan">Plan Mode</SelectItem>
                </SelectContent>
              </Select>
            </div>

            <div className="space-y-2">
              <label className="text-sm font-medium">{t('runMode.globalGoal')}</label>
              <textarea
                className="w-full min-h-16 rounded-md border border-input bg-background px-3 py-2 text-sm outline-none resize-y placeholder:text-muted-foreground focus-visible:border-primary/50 focus-visible:ring-2 focus-visible:ring-primary/10"
                value={globalGoal}
                onChange={(e) => setGlobalGoal(e.target.value)}
                placeholder={t('runMode.globalGoalPlaceholder')}
              />
            </div>
          </div>
        ) : (
          <div className="space-y-3">
            <div className="space-y-2">
              <label className="text-sm font-medium"><Workflow className="inline size-3.5 mr-1.5" />{t('runMode.workflowSection')}</label>
              <Select value={workflowTemplateId} onValueChange={setWorkflowTemplateId}>
                <SelectTrigger className="h-9">
                  <SelectValue placeholder={t('conversation.home.selectWorkflowTemplate')} />
                </SelectTrigger>
                <SelectContent>
                  {templates.map((tpl) => (
                    <SelectItem key={tpl.id} value={tpl.id}>{tpl.name}</SelectItem>
                  ))}
                  {templates.length === 0 ? (
                    <div className="px-2 py-3 text-xs text-muted-foreground">{t('conversation.home.noWorkflowTemplate')}</div>
                  ) : null}
                </SelectContent>
              </Select>
            </div>
          </div>
        )}

        <div className="flex items-center gap-3">
          <Button className="gap-2" onClick={handleSave}>
            <Save className="size-4" />
            {t('common.save')}
          </Button>
          {saved ? <span className="text-sm text-emerald-500">{t('runMode.saved')}</span> : null}
        </div>
      </div>
    </div>
  );
}
