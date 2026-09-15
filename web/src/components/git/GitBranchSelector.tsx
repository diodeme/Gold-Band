import { useCallback, useEffect, useRef, useState } from 'react';
import { Check, GitBranch, Loader2, Plus, TriangleAlert } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { changeGitBranch, getGitBranchPickerSnapshot, openExternalUrl } from '@/api';
import { displayAppError } from '@/i18n';
import type { GitBranchPickerSnapshotVm } from '@/types';
import { Button } from '@/components/ui/button';
import {
  Command,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
} from '@/components/ui/command';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { cn } from '@/lib/utils';
import { useOverflowTooltip } from '@/hooks/useOverflowTooltip';
import { GIT_DOWNLOAD_URL, isGitVersionCapabilityError } from '@/lib/git-capability';
import { useGitBranchPickerSnapshotStore } from './GitBranchPickerSnapshotContext';

export interface GitBranchSelectorProps {
  projectId: string;
  workspacePath?: string | null;
  disabled?: boolean;
  readOnlyBranch?: string | null;
  variant?: 'home' | 'session';
  responsiveContext?: boolean;
  onBranchChange?: (branch: string | null) => void;
  onMutationPendingChange?: (pending: boolean) => void;
}

export function GitBranchSelector({
  projectId,
  workspacePath,
  disabled = false,
  readOnlyBranch,
  variant = 'home',
  responsiveContext = false,
  onBranchChange,
  onMutationPendingChange,
}: GitBranchSelectorProps) {
  const { t } = useTranslation();
  const snapshotStore = useGitBranchPickerSnapshotStore();
  const scopeKey = `${projectId}\u0000${workspacePath ?? ''}`;
  const cachedSnapshot = readOnlyBranch === undefined
    ? snapshotStore.peek(projectId, workspacePath)
    : null;
  const [open, setOpen] = useState(false);
  const [pickerState, setPickerState] = useState<{
    scopeKey: string;
    snapshot: GitBranchPickerSnapshotVm | null;
    loading: boolean;
  }>(() => ({ scopeKey, snapshot: cachedSnapshot, loading: readOnlyBranch === undefined && !cachedSnapshot }));
  const [changing, setChanging] = useState(false);
  const [error, setError] = useState<{
    code: string | null;
    params: Record<string, unknown>;
    message: string;
  } | null>(null);
  const [createOpen, setCreateOpen] = useState(false);
  const [newBranchName, setNewBranchName] = useState('');
  const requestSequenceRef = useRef(0);
  const onBranchChangeRef = useRef(onBranchChange);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const popoverUsedPointerRef = useRef(false);
  const compactPointerClickPendingRef = useRef(false);
  const {
    valueRef: branchValueRef,
    tooltipOpen: branchTooltipOpen,
    showTooltipIfOverflowing: showBranchTooltipIfOverflowing,
    hideTooltip: hideBranchTooltip,
    handleTooltipOpenChange: handleBranchTooltipOpenChange,
  } = useOverflowTooltip<HTMLSpanElement>({ always: responsiveContext });
  const visiblePickerState = pickerState.scopeKey === scopeKey
    ? pickerState
    : { scopeKey, snapshot: cachedSnapshot, loading: readOnlyBranch === undefined && !cachedSnapshot };
  const snapshot = visiblePickerState.snapshot;
  const loading = visiblePickerState.loading;
  const handleBranchTooltipRootOpenChange = useCallback((next: boolean) => {
    if (!next && responsiveContext && compactPointerClickPendingRef.current) return;
    handleBranchTooltipOpenChange(next);
  }, [handleBranchTooltipOpenChange, responsiveContext]);

  useEffect(() => {
    onBranchChangeRef.current = onBranchChange;
  }, [onBranchChange]);

  const publishBranch = useCallback((next: GitBranchPickerSnapshotVm | null) => {
    onBranchChangeRef.current?.(next?.currentBranch ?? null);
  }, []);

  const loadSnapshot = useCallback(async () => {
    if (readOnlyBranch !== undefined) return;
    const sequence = ++requestSequenceRef.current;
    const cached = snapshotStore.get(projectId, workspacePath);
    setPickerState({ scopeKey, snapshot: cached, loading: !cached });
    publishBranch(cached);
    setError(null);
    try {
      const next = await getGitBranchPickerSnapshot(projectId, workspacePath);
      if (sequence !== requestSequenceRef.current) return;
      snapshotStore.set(projectId, workspacePath, next);
      setPickerState({ scopeKey, snapshot: next, loading: false });
      publishBranch(next);
    } catch (cause) {
      if (sequence !== requestSequenceRef.current) return;
      const candidate = cause && typeof cause === 'object'
        ? cause as { code?: unknown; params?: unknown }
        : null;
      const code = typeof candidate?.code === 'string' ? candidate.code : null;
      const params = candidate?.params && typeof candidate.params === 'object'
        ? candidate.params as Record<string, unknown>
        : {};
      const versionCapabilityError = isGitVersionCapabilityError(code);
      if (versionCapabilityError) snapshotStore.delete(projectId, workspacePath);
      const fallback = versionCapabilityError ? null : cached;
      setPickerState({ scopeKey, snapshot: fallback, loading: false });
      publishBranch(fallback);
      setError({ code, params, message: displayAppError(t, cause) });
    }
  }, [projectId, publishBranch, readOnlyBranch, scopeKey, snapshotStore, t, workspacePath]);

  useEffect(() => {
    void loadSnapshot();
    return () => {
      requestSequenceRef.current += 1;
    };
  }, [loadSnapshot]);

  useEffect(() => {
    onMutationPendingChange?.(changing);
    return () => onMutationPendingChange?.(false);
  }, [changing, onMutationPendingChange]);

  const applyChange = async (
    input: { kind: 'switch'; name: string } | { kind: 'create-and-switch'; name: string; startPoint: string },
  ) => {
    if (!snapshot || changing) return;
    setChanging(true);
    setError(null);
    try {
      const next = await changeGitBranch(projectId, workspacePath, {
        ...input,
        expectedRevision: snapshot.revision,
      });
      snapshotStore.set(projectId, workspacePath, next);
      setPickerState({ scopeKey, snapshot: next, loading: false });
      publishBranch(next);
      setOpen(false);
      setCreateOpen(false);
      setNewBranchName('');
    } catch (cause) {
      const message = displayAppError(t, cause);
      await loadSnapshot();
      setError({ code: null, params: {}, message });
    } finally {
      setChanging(false);
    }
  };

  if (readOnlyBranch !== undefined) {
    const branch = readOnlyBranch?.trim() || t('conversation.branchPicker.unavailable');
    return (
      <Tooltip>
        <TooltipTrigger asChild>
          <span
            tabIndex={0}
            className={cn(
              'flex h-7 min-w-0 max-w-44 items-center gap-1.5 rounded-md px-1.5 text-sm text-foreground/80 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring',
              variant === 'session' && 'max-w-36 text-xs',
            )}
            data-git-branch-selector="read-only"
          >
            <GitBranch className="size-3.5 shrink-0" />
            <span className="truncate">{branch}</span>
          </span>
        </TooltipTrigger>
        <TooltipContent side="top" sideOffset={6} className="max-w-[min(36rem,calc(100vw-2rem))] break-all">
          {branch}
        </TooltipContent>
      </Tooltip>
    );
  }

  const versionCapabilityError = isGitVersionCapabilityError(error?.code);
  const currentBranch = versionCapabilityError
    ? t('conversation.branchPicker.versionUnsupportedLabel')
    : snapshot?.currentBranch ?? t('conversation.branchPicker.unavailable');
  const blocked = disabled
    || changing
    || Boolean(snapshot?.lock.locked)
    || Boolean(snapshot?.operationInProgress);
  const operationLabel = snapshot?.operationInProgress
    ? t('conversation.branchPicker.operationInProgress', { operation: snapshot.operationInProgress.kind })
    : snapshot?.lock.locked
      ? t('conversation.branchPicker.locked')
      : null;

  return (
    <>
      <Tooltip open={branchTooltipOpen && !open} onOpenChange={handleBranchTooltipRootOpenChange}>
        <Popover open={open} onOpenChange={(next) => {
          compactPointerClickPendingRef.current = false;
          setOpen(next);
          hideBranchTooltip();
          if (next && !snapshot && !loading && !error) void loadSnapshot();
        }}>
          <TooltipTrigger asChild>
            <PopoverTrigger asChild>
              <Button
                ref={triggerRef}
                type="button"
                variant={null}
                size="sm"
                disabled={disabled}
                aria-label={`${t('conversation.branchPicker.label')}: ${currentBranch}`}
                data-git-branch-selector="editable"
                data-git-branch-popover-open={open ? 'true' : 'false'}
                onPointerEnter={showBranchTooltipIfOverflowing}
                onPointerLeave={() => {
                  compactPointerClickPendingRef.current = false;
                  hideBranchTooltip();
                }}
                onPointerDownCapture={(event) => {
                  compactPointerClickPendingRef.current = responsiveContext && event.button === 0;
                  popoverUsedPointerRef.current = event.button === 0;
                }}
                onKeyDownCapture={() => {
                  compactPointerClickPendingRef.current = false;
                  popoverUsedPointerRef.current = false;
                }}
                onPointerCancel={() => {
                  compactPointerClickPendingRef.current = false;
                  hideBranchTooltip();
                }}
                onClickCapture={() => {
                  compactPointerClickPendingRef.current = false;
                }}
                onFocus={showBranchTooltipIfOverflowing}
                onBlur={() => {
                  compactPointerClickPendingRef.current = false;
                  hideBranchTooltip();
                }}
                className={cn(
                  'h-7 min-w-0 max-w-44 gap-1.5 rounded-md px-1.5 text-sm font-normal shadow-none hover:bg-accent hover:text-accent-foreground focus-visible:ring-2 focus-visible:ring-primary/10 data-[git-branch-popover-open=true]:bg-accent data-[git-branch-popover-open=true]:text-accent-foreground has-[>svg]:px-1.5 dark:hover:bg-accent/50 dark:data-[git-branch-popover-open=true]:bg-accent/50',
                  variant === 'session' && 'max-w-36 text-xs text-foreground/80',
                  responsiveContext && 'w-7 shrink-0 justify-center gap-0 px-0 has-[>svg]:px-0 @md/conversation-context:w-auto @md/conversation-context:shrink @md/conversation-context:justify-start @md/conversation-context:gap-1.5 @md/conversation-context:px-1.5 @md/conversation-context:has-[>svg]:px-1.5',
                )}
              >
                {loading || changing ? <Loader2 className="size-3.5 shrink-0 animate-spin" /> : <GitBranch className="size-3.5 shrink-0" />}
                <span
                  ref={branchValueRef}
                  data-git-branch-value="true"
                  className={cn('truncate', responsiveContext && 'hidden @md/conversation-context:inline')}
                >
                  {currentBranch}
                </span>
              </Button>
            </PopoverTrigger>
          </TooltipTrigger>
          <PopoverContent
            align="start"
            sideOffset={6}
            data-git-branch-popover-align="start"
            className="w-[min(22rem,calc(100vw-2rem))] p-0"
            onPointerDownCapture={() => {
              popoverUsedPointerRef.current = true;
            }}
            onKeyDownCapture={() => {
              popoverUsedPointerRef.current = false;
            }}
            onCloseAutoFocus={(event) => {
              hideBranchTooltip();
              if (!popoverUsedPointerRef.current) return;
              event.preventDefault();
              triggerRef.current?.blur();
              popoverUsedPointerRef.current = false;
            }}
          >
            <Command>
              {!versionCapabilityError ? <CommandInput placeholder={t('conversation.branchPicker.search', { workspace: projectId })} /> : null}
              <CommandList className="max-h-72">
              {loading ? (
                <div className="flex items-center justify-center gap-2 px-3 py-6 text-xs text-muted-foreground">
                  <Loader2 className="size-3.5 animate-spin" />
                  {t('conversation.branchPicker.loading')}
                </div>
              ) : null}
              {!loading && error && !snapshot && !versionCapabilityError ? (
                <div className="space-y-2 px-3 py-3 text-xs text-destructive">
                  <p>{error.message}</p>
                  <Button type="button" variant="outline" size="xs" onClick={() => void loadSnapshot()}>
                    {t('common.retry')}
                  </Button>
                </div>
              ) : null}
              {!loading && error && versionCapabilityError ? (
                <div className="space-y-3 px-3 py-4" role="alert" data-git-version-capability-error={error.code}>
                  <div className="flex items-start gap-2">
                    <TriangleAlert className="mt-0.5 size-4 shrink-0 text-amber-600 dark:text-amber-400" />
                    <div className="min-w-0 space-y-1">
                      <p className="text-sm font-medium text-foreground">
                        {t(`conversation.branchPicker.${error.code === 'git.version-unsupported' ? 'versionUnsupportedTitle' : 'versionUnavailableTitle'}`)}
                      </p>
                      <p className="text-xs leading-relaxed text-muted-foreground">
                        {t(`conversation.branchPicker.${error.code === 'git.version-unsupported' ? 'versionUnsupportedDescription' : 'versionUnavailableDescription'}`, error.params)}
                      </p>
                    </div>
                  </div>
                  <div className="flex flex-wrap gap-2 pl-6">
                    <Button type="button" size="xs" onClick={() => void openExternalUrl(GIT_DOWNLOAD_URL)}>
                      {t('sourceControl.openGitDownload')}
                    </Button>
                    <Button type="button" variant="outline" size="xs" onClick={() => void loadSnapshot()}>
                      {t('sourceControl.checkAgain')}
                    </Button>
                  </div>
                </div>
              ) : null}
                {!loading && snapshot ? (
                  <>
                    <CommandEmpty>{t('conversation.branchPicker.empty')}</CommandEmpty>
                    <CommandGroup heading={t('conversation.branchPicker.branches')}>
                    {snapshot.branches.map((branch) => {
                      const current = branch.name === snapshot.currentBranch;
                      const usedElsewhere = branch.checkedOutWorktreePaths.some((path) => path !== snapshot.workspacePath);
                      return (
                        <CommandItem
                          key={branch.name}
                          value={branch.name}
                          disabled={blocked || current || usedElsewhere}
                          onSelect={() => void applyChange({ kind: 'switch', name: branch.name })}
                          className="items-start gap-2 py-2"
                        >
                          <GitBranch className="mt-0.5 size-4 shrink-0 text-muted-foreground" />
                          <span className="min-w-0 flex-1">
                            <span className="block truncate">{branch.name}</span>
                            {current && snapshot.dirtyFileCount > 0 ? (
                              <span className="block text-xs text-muted-foreground">
                                {t('conversation.branchPicker.dirtyFiles', { count: snapshot.dirtyFileCount })}
                              </span>
                            ) : usedElsewhere ? (
                              <span className="block truncate text-xs text-muted-foreground">
                                {t('conversation.branchPicker.checkedOutElsewhere')}
                              </span>
                            ) : null}
                          </span>
                          {current ? <Check className="mt-0.5 size-4 shrink-0" /> : null}
                        </CommandItem>
                      );
                    })}
                    </CommandGroup>
                  </>
                ) : null}
              </CommandList>
              {operationLabel ? (
                <div className="border-t border-border/60 px-3 py-2 text-xs text-muted-foreground" role="status">
                  {operationLabel}
                </div>
              ) : null}
              {error && snapshot ? (
                <div className="border-t border-destructive/20 px-3 py-2 text-xs text-destructive" role="alert">
                  {error.message}
                </div>
              ) : null}
              {!loading && snapshot ? (
                <div className="border-t border-border/60 p-1" data-git-branch-fixed-action="true">
                  <Button
                    type="button"
                    variant="ghost"
                    disabled={blocked || !snapshot.currentBranch}
                    className="h-9 w-full justify-start px-2 font-normal"
                    onClick={() => {
                      setOpen(false);
                      setCreateOpen(true);
                    }}
                  >
                    <Plus className="size-4" />
                    <span className="truncate">{t('conversation.branchPicker.createAndCheckout')}</span>
                  </Button>
                </div>
              ) : null}
            </Command>
          </PopoverContent>
        </Popover>
        <TooltipContent side="top" sideOffset={6} className="max-w-[min(36rem,calc(100vw-2rem))] break-all">
          {currentBranch}
        </TooltipContent>
      </Tooltip>

      <Dialog open={createOpen} onOpenChange={(next) => {
        if (!changing) setCreateOpen(next);
      }}>
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <DialogTitle className="text-base">{t('conversation.branchPicker.createTitle')}</DialogTitle>
            <DialogDescription>
              {t('conversation.branchPicker.createDescription', { branch: snapshot?.currentBranch ?? 'HEAD' })}
            </DialogDescription>
          </DialogHeader>
          <div className="grid gap-1.5">
            <Label htmlFor="conversation-new-branch">{t('sourceControl.name')}</Label>
            <Input
              id="conversation-new-branch"
              value={newBranchName}
              disabled={changing}
              autoFocus
              onChange={(event) => setNewBranchName(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === 'Enter' && newBranchName.trim() && snapshot?.currentBranch) {
                  event.preventDefault();
                  void applyChange({
                    kind: 'create-and-switch',
                    name: newBranchName.trim(),
                    startPoint: snapshot.currentBranch,
                  });
                }
              }}
            />
          </div>
          <DialogFooter>
            <Button type="button" variant="outline" disabled={changing} onClick={() => setCreateOpen(false)}>
              {t('common.cancel')}
            </Button>
            <Button
              type="button"
              disabled={changing || !newBranchName.trim() || !snapshot?.currentBranch}
              onClick={() => {
                if (!snapshot?.currentBranch) return;
                void applyChange({
                  kind: 'create-and-switch',
                  name: newBranchName.trim(),
                  startPoint: snapshot.currentBranch,
                });
              }}
            >
              {changing ? <Loader2 className="size-4 animate-spin" /> : null}
              {t('conversation.branchPicker.createAndCheckout')}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  );
}
