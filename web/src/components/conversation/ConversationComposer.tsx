import { useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Send, Paperclip, Workflow, Bot } from 'lucide-react';
import type { AgentRegistryVm, ConversationCreateInput, ConversationRunModeVm } from '../../types';
import { Button } from '@/components/ui/button';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { cn } from '@/lib/utils';

interface ConversationComposerProps {
  projectId: string;
  workspaceName: string;
  runMode: ConversationRunModeVm;
  agentRegistry: AgentRegistryVm | null;
  busy: boolean;
  onRunModeChange: (mode: ConversationRunModeVm) => void;
  onSubmit: (input: ConversationCreateInput) => void;
  onOpenRunModeSettings: () => void;
}

export function ConversationComposer({
  projectId,
  workspaceName,
  runMode,
  agentRegistry,
  busy,
  onRunModeChange,
  onSubmit,
  onOpenRunModeSettings,
}: ConversationComposerProps) {
  const { t } = useTranslation();
  const [content, setContent] = useState('');
  const [selectedAgent, setSelectedAgent] = useState(runMode.autoConfig?.agentType ?? '');
  const [selectedModel, setSelectedModel] = useState(runMode.autoConfig?.modelId ?? '');
  const [attachmentNames, setAttachmentNames] = useState<string[]>([]);
  const fileInputRef = useRef<HTMLInputElement>(null);

  const isAuto = runMode.mode === 'auto';
  const canSubmit = content.trim().length > 0 && !busy;
  const agents = agentRegistry?.agents.filter((a) => a.supported) ?? [];
  const selectedAgentObj = agents.find((a) => a.agentType === selectedAgent);
  const models: { id: string; name: string }[] = [];

  const handleFiles = () => {
    const input = fileInputRef.current;
    if (!input) return;
    const names: string[] = [];
    for (let i = 0; i < (input.files?.length ?? 0); i++) {
      const f = input.files?.item(i);
      if (f) names.push(f.name);
    }
    setAttachmentNames(names);
  };

  const handleSubmit = () => {
    if (!canSubmit) return;
    onSubmit({
      projectId,
      content: content.trim(),
      runMode: runMode.mode,
      workflowTemplateId: isAuto ? undefined : runMode.workflowTemplateId ?? undefined,
      autoConfig: isAuto
        ? { agentType: selectedAgent, modelId: selectedModel || undefined }
        : undefined,
    });
    setContent('');
    setAttachmentNames([]);
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      handleSubmit();
    }
  };

  return (
    <div className="flex flex-col gap-4">
      {/* Main text input */}
      <div className="rounded-2xl border border-border/60 bg-card/60 p-4 shadow-sm transition-colors focus-within:border-primary/40 focus-within:ring-2 focus-within:ring-primary/10">
        <textarea
          className="w-full min-h-20 resize-none bg-transparent text-sm leading-6 text-foreground placeholder:text-muted-foreground outline-none"
          placeholder={t('conversation.home.inputPlaceholder')}
          value={content}
          onChange={(e) => setContent(e.target.value)}
          onKeyDown={handleKeyDown}
          disabled={busy}
        />
        <div className="mt-3 flex items-center justify-between border-t border-border/50 pt-3">
          <div className="flex items-center gap-3">
            <input
              ref={fileInputRef}
              type="file"
              multiple
              className="hidden"
              onChange={handleFiles}
            />
            <Button
              variant="ghost"
              size="icon"
              className="size-8 text-muted-foreground"
              onClick={() => fileInputRef.current?.click()}
              disabled={busy}
            >
              <Paperclip className="size-4" />
            </Button>
            {attachmentNames.length > 0 ? (
              <span className="text-xs text-muted-foreground">{attachmentNames.join(', ')}</span>
            ) : (
              <span className="text-xs text-muted-foreground">{workspaceName}</span>
            )}
          </div>
          <Button size="sm" className="h-8 gap-1.5 rounded-full px-3" disabled={!canSubmit} onClick={handleSubmit}>
            <Send className="size-3.5" />
            {t('acp.send')}
          </Button>
        </div>
      </div>

      {/* Run mode selector */}
      <div className="flex items-center gap-3 rounded-xl border border-border/50 bg-card/40 px-4 py-3">
        <span className="text-xs font-medium text-muted-foreground">{t('conversation.home.runMode')}</span>
        <div className="flex rounded-lg bg-muted p-0.5">
          <button
            type="button"
            className={cn(
              'rounded-md px-3 py-1 text-xs font-medium transition-colors',
              isAuto ? 'bg-background text-foreground shadow-sm' : 'text-muted-foreground hover:text-foreground',
            )}
            onClick={() => onRunModeChange({ mode: 'auto', autoConfig: runMode.autoConfig })}
          >
            {t('conversation.home.auto')}
          </button>
          <button
            type="button"
            className={cn(
              'rounded-md px-3 py-1 text-xs font-medium transition-colors',
              !isAuto ? 'bg-background text-foreground shadow-sm' : 'text-muted-foreground hover:text-foreground',
            )}
            onClick={() => onRunModeChange({ mode: 'workflow', workflowTemplateId: runMode.workflowTemplateId })}
          >
            {t('conversation.home.workflow')}
          </button>
        </div>

        {/* Configure button */}
        <Button variant="ghost" size="sm" className="ml-auto h-7 gap-1 text-xs" onClick={onOpenRunModeSettings}>
          <Workflow className="size-3" />
          {t('conversation.home.configureNow')}
        </Button>
      </div>

      {/* AUTO options: agent + model */}
      {isAuto ? (
        <div className="flex items-center gap-3 rounded-xl border border-border/50 bg-card/40 px-4 py-3">
          <Bot className="size-4 text-muted-foreground" />
          <div className="flex items-center gap-3 flex-1">
            <Select value={selectedAgent} onValueChange={(v) => { setSelectedAgent(v); setSelectedModel(''); }}>
              <SelectTrigger className="h-8 w-[180px] text-xs">
                <SelectValue placeholder={t('conversation.home.selectAgent')} />
              </SelectTrigger>
              <SelectContent>
                {agents.map((a) => (
                  <SelectItem key={a.agentType} value={a.agentType}>{a.displayName}</SelectItem>
                ))}
                {agents.length === 0 ? (
                  <div className="px-2 py-3 text-xs text-muted-foreground">{t('conversation.home.noAgent')}</div>
                ) : null}
              </SelectContent>
            </Select>
            {selectedAgentObj && models.length > 0 ? (
              <Select value={selectedModel} onValueChange={setSelectedModel}>
                <SelectTrigger className="h-8 w-[180px] text-xs">
                  <SelectValue placeholder={t('conversation.home.selectModel')} />
                </SelectTrigger>
                <SelectContent>
                  {models.map((m) => (
                    <SelectItem key={m.id} value={m.id}>{m.name}</SelectItem>
                  ))}
                </SelectContent>
              </Select>
            ) : null}
          </div>
        </div>
      ) : null}
    </div>
  );
}
