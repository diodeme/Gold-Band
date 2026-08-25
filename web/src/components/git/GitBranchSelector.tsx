import { useCallback, useEffect, useRef, useState } from 'react';
import { Check, GitBranch, Loader2, Plus } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { changeGitBranch, getGitBranchPickerSnapshot } from '@/api';
import { displayAppError } from '@/i18n';
import type { GitBranchCheckpointVm, GitBranchPickerSnapshotVm } from '@/types';
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
import { useGitBranchPickerSnapshotStore } from './GitBranchPickerSnapshotContext';

export interface GitBranchSelectorProps {
  projectId: string;
  workspacePath?: string | null;
  disabled?: boolean;
  readOnlyBranch?: string | null;
  variant?: 'home' | 'session';
  onCheckpointChange?: (checkpoint: GitBranchCheckpointVm | null) => void;
  onMutationPendingChange?: (pending: boolean) => void;
}

export function GitBranchSelector({
  projectId,
  workspacePath,
  disabled = false,
  readOnlyBranch,
  variant = 'home',
  onCheckpointChange,
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
  const [error, setError] = useState<string | null>(null);
  const [createOpen, setCreateOpen] = useState(false);
  const [newBranchName, setNewBranchName] = useState('');
  const requestSequenceRef = useRef(0);
  const {
    valueRef: branchValueRef,
    tooltipOpen: branchTooltipOpen,
    showTooltipIfOverflowing: showBranchTooltipIfOverflowing,
    hideTooltip: hideBranchTooltip,
    handleTooltipOpenChange: handleBranchTooltipOpenChange,
  } = useOverflowTooltip<HTMLSpanElement>();
  const visiblePickerState = pickerState.scopeKey === scopeKey
    ? pickerState
    : { scopeKey, snapshot: cachedSnapshot, loading: readOnlyBranch === undefined && !cachedSnapshot };
  const snapshot = visiblePickerState.snapshot;
  const loading = visiblePickerState.loading;

  const publishCheckpoint = useCallback((next: GitBranchPickerSnapshotVm | null) => {
    onCheckpointChange?.(
      next?.currentBranch && next.headOid
        ? { branch: next.currentBranch, headOid: next.headOid, revision: next.revision }
        : null,
    );
  }, [onCheckpointChange]);

  const loadSnapshot = useCallback(async () => {
    if (readOnlyBranch !== undefined) return;
    const sequence = ++requestSequenceRef.current;
    const cached = snapshotStore.get(projectId, workspacePath);
    setPickerState({ scopeKey, snapshot: cached, loading: !cached });
    publishCheckpoint(cached);
    setError(null);
    try {
      const next = await getGitBranchPickerSnapshot(projectId, workspacePath);
      if (sequence !== requestSequenceRef.current) return;
      snapshotStore.set(projectId, workspacePath, next);
      setPickerState({ scopeKey, snapshot: next, loading: false });
      publishCheckpoint(next);
    } catch (cause) {
      if (sequence !== requestSequenceRef.current) return;
      setPickerState({ scopeKey, snapshot: cached, loading: false });
      publishCheckpoint(cached);
      setError(displayAppError(t, cause));
    }
  }, [projectId, publishCheckpoint, readOnlyBranch, scopeKey, snapshotStore, t, workspacePath]);

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
      publishCheckpoint(next);
      setOpen(false);
      setCreateOpen(false);
      setNewBranchName('');
    } catch (cause) {
      const message = displayAppError(t, cause);
      await loadSnapshot();
      setError(message);
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

  const currentBranch = snapshot?.currentBranch ?? t('conversation.branchPicker.unavailable');
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
      <Tooltip open={branchTooltipOpen} onOpenChange={handleBranchTooltipOpenChange}>
        <Popover open={open} onOpenChange={(next) => {
          setOpen(next);
          hideBranchTooltip();
          if (next && !snapshot && !loading) void loadSnapshot();
        }}>
          <TooltipTrigger asChild>
            <PopoverTrigger asChild>
              <Button
                type="button"
                variant={null}
                size="sm"
                disabled={disabled}
                aria-label={t('conversation.branchPicker.label')}
                data-git-branch-selector="editable"
                onPointerEnter={showBranchTooltipIfOverflowing}
                onPointerLeave={hideBranchTooltip}
                onPointerDown={hideBranchTooltip}
                onFocus={showBranchTooltipIfOverflowing}
                onBlur={hideBranchTooltip}
                className={cn(
                  'h-7 min-w-0 max-w-44 gap-1.5 rounded-md px-1.5 text-sm font-normal shadow-none hover:bg-accent hover:text-accent-foreground focus-visible:ring-2 focus-visible:ring-primary/10 has-[>svg]:px-1.5 dark:hover:bg-accent/50',
                  variant === 'session' && 'max-w-36 text-xs text-foreground/80',
                )}
              >
                {loading || changing ? <Loader2 className="size-3.5 shrink-0 animate-spin" /> : <GitBranch className="size-3.5 shrink-0" />}
                <span ref={branchValueRef} data-git-branch-value="true" className="truncate">{currentBranch}</span>
              </Button>
            </PopoverTrigger>
          </TooltipTrigger>
          <PopoverContent
            align="start"
            sideOffset={6}
            data-git-branch-popover-align="start"
            className="w-[min(22rem,calc(100vw-2rem))] p-0"
          >
            <Command>
              <CommandInput placeholder={t('conversation.branchPicker.search', { workspace: projectId })} />
              <CommandList className="max-h-72">
              {loading ? (
                <div className="flex items-center justify-center gap-2 px-3 py-6 text-xs text-muted-foreground">
                  <Loader2 className="size-3.5 animate-spin" />
                  {t('conversation.branchPicker.loading')}
                </div>
              ) : null}
              {!loading && error && !snapshot ? (
                <div className="space-y-2 px-3 py-3 text-xs text-destructive">
                  <p>{error}</p>
                  <Button type="button" variant="outline" size="xs" onClick={() => void loadSnapshot()}>
                    {t('common.retry')}
                  </Button>
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
                  {error}
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
