import { useTranslation } from 'react-i18next';

import { Button } from '@/components/ui/button';
import { Checkbox } from '@/components/ui/checkbox';
import { Label } from '@/components/ui/label';
import { agentIconClass, agentIconSrc } from '@/lib/agent-icons';
import type { ConfiguredSkillAgentMeta } from '@/lib/skill-agent-display';

interface SkillSyncTargetSelectorProps {
  agents: ConfiguredSkillAgentMeta[];
  value: string[];
  onValueChange: (value: string[]) => void;
}

export function SkillSyncTargetSelector({ agents, value, onValueChange }: SkillSyncTargetSelectorProps) {
  const { t } = useTranslation();
  const selectedAgentTypes = new Set(value);
  const allSelected = agents.length > 0 && agents.every((agent) => selectedAgentTypes.has(agent.agentType));
  const noneSelected = agents.every((agent) => !selectedAgentTypes.has(agent.agentType));

  const changeTarget = (agentType: string, checked: boolean) => {
    const nextSelected = new Set(value);
    if (checked) nextSelected.add(agentType);
    else nextSelected.delete(agentType);
    onValueChange(agents.filter((agent) => nextSelected.has(agent.agentType)).map((agent) => agent.agentType));
  };

  return (
    <fieldset className="space-y-2">
      <legend className="sr-only">{t('contextManagement.skills.syncTargets', '同步到')}</legend>
      <div className="flex items-center justify-between gap-3">
        <span aria-hidden="true" className="text-sm font-medium">{t('contextManagement.skills.syncTargets', '同步到')}</span>
        <div className="flex items-center gap-1">
          <Button
            type="button"
            variant="ghost"
            size="xs"
            disabled={agents.length === 0 || allSelected}
            onClick={() => onValueChange(agents.map((agent) => agent.agentType))}
          >
            {t('contextManagement.skills.selectAllSyncTargets', '全选')}
          </Button>
          <Button
            type="button"
            variant="ghost"
            size="xs"
            disabled={agents.length === 0 || noneSelected}
            onClick={() => onValueChange([])}
          >
            {t('contextManagement.skills.selectNoSyncTargets', '全不选')}
          </Button>
        </div>
      </div>
      {agents.length > 0 ? (
        <div className="flex flex-wrap gap-x-4 gap-y-2">
          {agents.map((agent) => {
            const inputId = `skill-sync-target-${agent.agentType}`;
            return (
              <div key={agent.agentType} className="flex items-center gap-1.5">
                <Checkbox
                  id={inputId}
                  checked={selectedAgentTypes.has(agent.agentType)}
                  onCheckedChange={(checked) => changeTarget(agent.agentType, checked === true)}
                />
                <Label htmlFor={inputId} className="gap-1.5 font-normal">
                  <img src={agentIconSrc(agent.iconKey)} alt="" className={agentIconClass(agent.iconKey, 'size-4')} />
                  <span className="text-sm">{agent.label}</span>
                </Label>
              </div>
            );
          })}
        </div>
      ) : (
        <p className="text-xs text-muted-foreground">{t('contextManagement.skills.noConfiguredAgents', '没有可同步的已配置 Agent。')}</p>
      )}
    </fieldset>
  );
}
