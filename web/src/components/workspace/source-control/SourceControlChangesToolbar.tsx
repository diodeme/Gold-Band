import { useState, type ReactNode } from 'react';
import {
  Archive,
  CheckCheck,
  CloudDownload,
  Ellipsis,
  LoaderCircle,
  Undo2,
} from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Button } from '@/components/ui/button';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { Switch } from '@/components/ui/switch';
import type {
  GitMutationRequestVm,
  GitOperationRequestVm,
  GitPullStrategyVm,
  GitSourceControlSnapshotVm,
} from '@/types';
import { rememberPreferredGitRemote, resolvePreferredGitRemote } from './source-control-preferences';

type ChangesActionKind = 'fetch' | 'pull' | 'push' | 'stash-create';

export function SourceControlSyncActions({ snapshot, busyActionKind, locked, onOperation }: {
  snapshot: GitSourceControlSnapshotVm;
  busyActionKind: string | null;
  locked: boolean;
  onOperation: (input: GitOperationRequestVm) => void;
}) {
  const { t } = useTranslation();
  const [action, setAction] = useState<Exclude<ChangesActionKind, 'stash-create'> | null>(null);
  const [flag, setFlag] = useState(false);
  const [remote, setRemote] = useState('');
  const [pullStrategy, setPullStrategy] = useState<GitPullStrategyVm>('fast-forward-only');
  const busy = busyActionKind !== null;
  const remoteNames = snapshot.repository.remotes.map((item) => item.name);
  const defaultRemote = resolvePreferredGitRemote(
    snapshot.repository.commonDir,
    remoteNames,
    snapshot.repository.upstream?.name,
  );
  const openAction = (next: Exclude<ChangesActionKind, 'stash-create'>) => {
    setAction(next);
    setFlag(false);
    setRemote(defaultRemote);
    setPullStrategy('fast-forward-only');
  };
  const selectRemote = (value: string) => {
    setRemote(value);
    rememberPreferredGitRemote(snapshot.repository.commonDir, value);
  };
  const submit = () => {
    if (action === 'fetch') onOperation({ kind: 'fetch', remote: remote || null, prune: flag });
    if (action === 'pull') onOperation({ kind: 'pull', remote: null, branch: null, strategy: pullStrategy });
    if (action === 'push') {
      const branch = snapshot.repository.currentBranch;
      if (!branch || !remote) return;
      onOperation({ kind: 'push', remote, branch, setUpstream: !snapshot.repository.upstream });
    }
    setAction(null);
  };
  const valid = action === 'pull' || Boolean(action && remote && (action !== 'push' || snapshot.repository.currentBranch));
  const ahead = snapshot.repository.upstream?.ahead ?? 0;
  const behind = snapshot.repository.upstream?.behind ?? 0;
  const syncAction = behind > 0 ? 'pull' : 'push';
  const canSync = syncAction === 'pull'
    ? Boolean(snapshot.repository.upstream)
    : Boolean(defaultRemote && snapshot.repository.currentBranch)
      && (!snapshot.repository.upstream || ahead > 0);
  const syncLabel = t(`sourceControl.${syncAction}`);
  return (
    <>
      <Tooltip>
        <TooltipTrigger asChild>
          <Button type="button" size="xs" variant="ghost" className="gap-1 px-1 text-muted-foreground" disabled={busy || locked || !canSync} aria-label={syncLabel} onClick={() => openAction(syncAction)}>
            {busyActionKind === syncAction ? <LoaderCircle className="size-3 animate-spin" /> : null}
            {behind > 0 ? <span className="tabular-nums" aria-hidden="true">↓{behind}</span> : null}
            {snapshot.repository.upstream ? <span className="tabular-nums" aria-hidden="true">↑{ahead}</span> : <span aria-hidden="true">↑</span>}
          </Button>
        </TooltipTrigger>
        <TooltipContent>{syncLabel}</TooltipContent>
      </Tooltip>
      <Tooltip>
        <TooltipTrigger asChild>
          <Button type="button" size="icon-xs" variant="ghost" disabled={busy || !defaultRemote} aria-label={t('sourceControl.fetch')} onClick={() => openAction('fetch')}>
            {busyActionKind === 'fetch' ? <LoaderCircle className="size-3.5 animate-spin" /> : <CloudDownload className="size-3.5" />}
          </Button>
        </TooltipTrigger>
        <TooltipContent>{t('sourceControl.fetch')}</TooltipContent>
      </Tooltip>
      <ChangesActionDialog action={action} message="" flag={flag} remote={remote} pullStrategy={pullStrategy} remotes={remoteNames} onMessage={() => undefined} onFlag={setFlag} onRemote={selectRemote} onPullStrategy={setPullStrategy} onClose={() => setAction(null)} onSubmit={submit} valid={valid} />
    </>
  );
}

export function SourceControlChangesToolbar({
  snapshot,
  busyActionKind,
  locked,
  onMutation,
  onOperation,
}: {
  snapshot: GitSourceControlSnapshotVm;
  busyActionKind: string | null;
  locked: boolean;
  onMutation: (input: GitMutationRequestVm) => void;
  onOperation: (input: GitOperationRequestVm) => void;
}) {
  const { t } = useTranslation();
  const [action, setAction] = useState<'stash-create' | null>(null);
  const [message, setMessage] = useState('');
  const [flag, setFlag] = useState(false);
  const busy = busyActionKind !== null;
  const hasWorkspaceChanges = snapshot.status.conflicts.length
    + snapshot.status.staged.length
    + snapshot.status.unstaged.length
    + snapshot.status.untracked.length > 0;

  const openAction = (next: 'stash-create') => {
    setAction(next);
    setMessage('');
    setFlag(false);
  };

  const submit = () => {
    switch (action) {
      case 'stash-create':
        onOperation({ kind: 'stash-create', message: message.trim() || null, includeUntracked: flag });
        break;
      case null:
        return;
    }
    setAction(null);
  };

  return (
    <>
      <div className="flex h-8 shrink-0 items-center border-b border-border/50 px-2" data-source-control-changes-toolbar="true">
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button className="ml-auto" size="icon-xs" variant="ghost" disabled={busy} aria-label={t('sourceControl.changeActions')}>
              <Ellipsis className="size-3.5" />
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end">
            <DropdownMenuItem disabled={locked || snapshot.status.unstaged.length + snapshot.status.untracked.length === 0} onSelect={() => onMutation({ kind: 'stage-all' })}>
              <CheckCheck />{t('sourceControl.stageAll')}
            </DropdownMenuItem>
            <DropdownMenuItem disabled={locked || snapshot.status.staged.length === 0} onSelect={() => onMutation({ kind: 'unstage-all' })}>
              <Undo2 />{t('sourceControl.unstageAll')}
            </DropdownMenuItem>
            <DropdownMenuSeparator />
            <DropdownMenuItem disabled={locked || !hasWorkspaceChanges} onSelect={() => openAction('stash-create')}>
              <Archive />{t('sourceControl.createStash')}
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      </div>
      <ChangesActionDialog
        action={action}
        message={message}
        flag={flag}
        remote=""
        pullStrategy="fast-forward-only"
        remotes={[]}
        onMessage={setMessage}
        onFlag={setFlag}
        onRemote={() => undefined}
        onPullStrategy={() => undefined}
        onClose={() => setAction(null)}
        onSubmit={submit}
        valid
      />
    </>
  );
}

function ChangesActionDialog({ action, message, flag, remote, pullStrategy, remotes, onMessage, onFlag, onRemote, onPullStrategy, onClose, onSubmit, valid }: {
  action: ChangesActionKind | null;
  message: string;
  flag: boolean;
  remote: string;
  pullStrategy: GitPullStrategyVm;
  remotes: string[];
  onMessage: (value: string) => void;
  onFlag: (value: boolean) => void;
  onRemote: (value: string) => void;
  onPullStrategy: (value: GitPullStrategyVm) => void;
  onClose: () => void;
  onSubmit: () => void;
  valid: boolean;
}) {
  const { t } = useTranslation();
  if (!action) return null;
  const showRemote = action === 'fetch' || action === 'push';
  return (
    <Dialog open onOpenChange={(open) => { if (!open) onClose(); }}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle className="text-base">{t(`sourceControl.actions.${action}.title`)}</DialogTitle>
          <DialogDescription>{t(`sourceControl.actions.${action}.description`)}</DialogDescription>
        </DialogHeader>
        <div className="grid gap-3">
          {showRemote ? <Field label={t('sourceControl.remote')}><Select value={remote} onValueChange={onRemote}><SelectTrigger className="w-full"><SelectValue /></SelectTrigger><SelectContent>{remotes.map((item) => <SelectItem key={item} value={item}>{item}</SelectItem>)}</SelectContent></Select></Field> : null}
          {action === 'stash-create' ? <Field label={t('sourceControl.messageOptional')}><Input value={message} onChange={(event) => onMessage(event.target.value)} /></Field> : null}
          {action === 'pull' ? <Field label={t('sourceControl.pullStrategy')}><Select value={pullStrategy} onValueChange={(value) => onPullStrategy(value as GitPullStrategyVm)}><SelectTrigger className="w-full"><SelectValue /></SelectTrigger><SelectContent><SelectItem value="fast-forward-only">{t('sourceControl.ffOnly')}</SelectItem><SelectItem value="merge">{t('sourceControl.merge')}</SelectItem><SelectItem value="rebase">{t('sourceControl.rebase')}</SelectItem></SelectContent></Select></Field> : null}
          {action === 'fetch' ? <ToggleRow label={t('sourceControl.prune')} description={t('sourceControl.pruneDescription')} checked={flag} onChecked={onFlag} /> : null}
          {action === 'stash-create' ? <ToggleRow label={t('sourceControl.includeUntracked')} checked={flag} onChecked={onFlag} /> : null}
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={onClose}>{t('common.cancel')}</Button>
          <Button disabled={!valid} onClick={onSubmit}>{t('common.confirm')}</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function Field({ label, children }: { label: string; children: ReactNode }) {
  return <div className="grid gap-1.5"><Label>{label}</Label>{children}</div>;
}

function ToggleRow({ label, description, checked, onChecked }: { label: string; description?: string; checked: boolean; onChecked: (checked: boolean) => void }) {
  return <div className="flex items-center justify-between gap-3 rounded-md border border-border/60 px-3 py-2"><div className="min-w-0"><Label>{label}</Label>{description ? <p className="mt-0.5 text-xs text-muted-foreground">{description}</p> : null}</div><Switch checked={checked} onCheckedChange={onChecked} /></div>;
}
