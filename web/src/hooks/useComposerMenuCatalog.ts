import { useCallback, useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import { useAgentCommands } from '@/hooks/useAgentCommands';
import { revalidateProfileCatalog, useProfileCatalog } from '@/lib/profile-catalog';
import { buildSlashCatalog, type ComposerMenuTrigger } from '@/lib/slash-command';
import type { ProfileVm } from '@/types';

const NO_PROFILES: readonly ProfileVm[] = [];

interface ComposerMenuCatalogOptions {
  agentType: string | null | undefined;
  workspacePath: string | null | undefined;
  sessionCommands?: readonly unknown[] | null;
  /** `@` roles are a Direct conversation input; Workflow and AUTO nodes use their own profiles. */
  rolesEnabled: boolean;
}

/**
 * The `/` Agent commands and Skills plus the `@` roles a composer menu offers.
 * Skills and roles change on disk or in other pages, so opening a menu re-reads
 * its catalog while the last list stays visible.
 */
export function useComposerMenuCatalog({
  agentType,
  workspacePath,
  sessionCommands,
  rolesEnabled,
}: ComposerMenuCatalogOptions) {
  const { t } = useTranslation();
  const agentCommands = useAgentCommands(agentType, workspacePath, sessionCommands);
  const profileCatalog = useProfileCatalog(rolesEnabled);
  const profiles = rolesEnabled ? profileCatalog.profiles : NO_PROFILES;
  const groups = useMemo(
    () => buildSlashCatalog(t('acp.slashAgentGroup'), profiles, agentCommands.commands),
    [agentCommands.commands, profiles, t],
  );
  const { revalidate } = agentCommands;
  const onMenuTrigger = useCallback((trigger: ComposerMenuTrigger) => {
    if (trigger === '/') revalidate();
    else if (rolesEnabled) void revalidateProfileCatalog().catch(() => undefined);
  }, [revalidate, rolesEnabled]);
  return { groups, contextKey: agentCommands.catalogKey, onMenuTrigger };
}
