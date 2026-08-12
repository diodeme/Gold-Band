import { useMemo, useState, type ReactNode } from 'react';
import {
  Archive,
  CloudDownload,
  CloudUpload,
  Ellipsis,
  FileCode2,
  GitBranch,
  GitPullRequestArrow,
  LoaderCircle,
  Plus,
  Tags,
} from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Button } from '@/components/ui/button';
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
import { ScrollArea } from '@/components/ui/scroll-area';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { Separator } from '@/components/ui/separator';
import { Switch } from '@/components/ui/switch';
import type {
  GitMutationRequestVm,
  GitOperationRequestVm,
  GitPullStrategyVm,
  GitSourceControlSnapshotVm,
} from '@/types';
import { rememberPreferredGitRemote, resolvePreferredGitRemote } from './source-control-preferences';

type RepositoryActionKind =
  | 'fetch'
  | 'pull'
  | 'push'
  | 'branch-create'
  | 'branch-rename'
  | 'branch-delete'
  | 'tag-create'
  | 'tag-delete'
  | 'push-tag'
  | 'worktree-create'
  | 'stash-create'
  | 'stash-apply';

type RepositoryAction = { kind: RepositoryActionKind; target?: string };

export function SourceControlRepositoryView({
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
  const [action, setAction] = useState<RepositoryAction | null>(null);
  const [name, setName] = useState('');
  const [target, setTarget] = useState('HEAD');
  const [message, setMessage] = useState('');
  const [path, setPath] = useState('');
  const [flag, setFlag] = useState(false);
  const [style, setStyle] = useState<'annotated' | 'lightweight'>('annotated');
  const [remote, setRemote] = useState('');
  const [pullStrategy, setPullStrategy] = useState<GitPullStrategyVm>('fast-forward-only');
  const busy = busyActionKind !== null;
  const localBranches = snapshot.refs.filter((ref) => ref.kind === 'local-branch');
  const tags = snapshot.refs.filter((ref) => ref.kind === 'tag');
  const remoteNames = snapshot.repository.remotes.map((item) => item.name);
  const defaultRemote = resolvePreferredGitRemote(
    snapshot.repository.commonDir,
    remoteNames,
    snapshot.repository.upstream?.name,
  );

  const openAction = (next: RepositoryAction) => {
    setAction(next);
    setName(next.target ?? '');
    setTarget('HEAD');
    setMessage('');
    setPath('');
    setFlag(false);
    setStyle('annotated');
    setRemote(defaultRemote);
    setPullStrategy('fast-forward-only');
  };

  const selectRemote = (value: string) => {
    setRemote(value);
    rememberPreferredGitRemote(snapshot.repository.commonDir, value);
  };

  const submit = () => {
    if (!action) return;
    switch (action.kind) {
      case 'fetch':
        onOperation({ kind: 'fetch', remote: remote || null, prune: flag });
        break;
      case 'pull':
        onOperation({ kind: 'pull', remote: null, branch: null, strategy: pullStrategy });
        break;
      case 'push': {
        const branch = snapshot.repository.currentBranch;
        if (!branch || !remote) return;
        onOperation({ kind: 'push', remote, branch, setUpstream: !snapshot.repository.upstream });
        break;
      }
      case 'branch-create':
        onMutation({ kind: 'branch-create', name: name.trim(), startPoint: target.trim() || 'HEAD', checkout: flag });
        break;
      case 'branch-rename':
        onMutation({ kind: 'branch-rename', oldName: action.target, newName: name.trim() });
        break;
      case 'branch-delete':
        onMutation({ kind: 'branch-delete-safe', name: action.target ?? '' });
        break;
      case 'tag-create':
        onMutation({ kind: 'tag-create', name: name.trim(), target: target.trim() || 'HEAD', style, message: message.trim() || null });
        break;
      case 'tag-delete':
        onMutation({ kind: 'tag-delete-local', name: action.target ?? '' });
        break;
      case 'push-tag':
        if (remote) onOperation({ kind: 'push-tag', remote, tag: action.target ?? '' });
        break;
      case 'worktree-create':
        onMutation({ kind: 'worktree-create', path: path.trim(), sourceRef: target.trim() || 'HEAD', newBranch: name.trim() || null });
        break;
      case 'stash-create':
        onOperation({ kind: 'stash-create', message: message.trim() || null, includeUntracked: flag });
        break;
      case 'stash-apply':
        onOperation({ kind: 'stash-apply', stashRef: action.target ?? '', restoreIndex: flag });
        break;
    }
    setAction(null);
  };

  const valid = useMemo(() => {
    if (!action) return false;
    switch (action.kind) {
      case 'push': return Boolean(remote && snapshot.repository.currentBranch);
      case 'branch-create':
      case 'branch-rename':
      case 'tag-create': return name.trim().length > 0;
      case 'worktree-create': return path.trim().length > 0;
      case 'push-tag': return remote.length > 0;
      default: return true;
    }
  }, [action, name, path, remote, snapshot.repository.currentBranch]);

  return (
    <div className="flex min-h-0 flex-1 flex-col" data-source-control-repository="true">
      <div className="flex h-10 shrink-0 items-center gap-1 border-b border-border/50 px-2">
        <Button size="sm" variant="ghost" disabled={busy || !defaultRemote} onClick={() => openAction({ kind: 'fetch' })}>
          {busyActionKind === 'fetch' ? <LoaderCircle className="size-3.5 animate-spin" /> : <CloudDownload className="size-3.5" />}{t('sourceControl.fetch')}
        </Button>
        <Button size="sm" variant="ghost" disabled={busy || locked || !snapshot.repository.upstream} onClick={() => openAction({ kind: 'pull' })}>
          {busyActionKind === 'pull' ? <LoaderCircle className="size-3.5 animate-spin" /> : <GitPullRequestArrow className="size-3.5" />}{t('sourceControl.pull')}
        </Button>
        <Button size="sm" variant="ghost" disabled={busy || !defaultRemote || !snapshot.repository.currentBranch} onClick={() => openAction({ kind: 'push' })}>
          {busyActionKind === 'push' ? <LoaderCircle className="size-3.5 animate-spin" /> : <CloudUpload className="size-3.5" />}{t('sourceControl.push')}
        </Button>
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button className="ml-auto" size="icon-xs" variant="ghost" disabled={busy} aria-label={t('sourceControl.repositoryActions')}><Plus className="size-3.5" /></Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end">
            <DropdownMenuItem onSelect={() => openAction({ kind: 'branch-create' })}>{t('sourceControl.createBranch')}</DropdownMenuItem>
            <DropdownMenuItem onSelect={() => openAction({ kind: 'tag-create' })}>{t('sourceControl.createTag')}</DropdownMenuItem>
            <DropdownMenuItem onSelect={() => openAction({ kind: 'worktree-create' })}>{t('sourceControl.createWorktree')}</DropdownMenuItem>
            <DropdownMenuSeparator />
            <DropdownMenuItem disabled={locked} onSelect={() => openAction({ kind: 'stash-create' })}>{t('sourceControl.createStash')}</DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      </div>
      <ScrollArea className="min-h-0 flex-1">
        <RepositorySection icon={<GitBranch className="size-3.5" />} title={t('sourceControl.branches')}>
          {localBranches.map((ref) => (
            <RepositoryRow key={ref.fullName} primary={ref.shortName} secondary={ref.targetOid.slice(0, 8)} active={ref.shortName === snapshot.repository.currentBranch}>
              <DropdownMenu>
                <DropdownMenuTrigger asChild><Button size="icon-xs" variant="ghost" disabled={busy}><Ellipsis className="size-3.5" /></Button></DropdownMenuTrigger>
                <DropdownMenuContent align="end">
                  <DropdownMenuItem disabled={locked || ref.shortName === snapshot.repository.currentBranch || ref.checkedOutWorktreePaths.length > 0} onSelect={() => onMutation({ kind: 'branch-switch', name: ref.shortName })}>{t('sourceControl.switchBranch')}</DropdownMenuItem>
                  <DropdownMenuItem disabled={locked} onSelect={() => openAction({ kind: 'branch-rename', target: ref.shortName })}>{t('sourceControl.rename')}</DropdownMenuItem>
                  <DropdownMenuSeparator />
                  <DropdownMenuItem variant="destructive" disabled={ref.shortName === snapshot.repository.currentBranch || ref.checkedOutWorktreePaths.length > 0} onSelect={() => openAction({ kind: 'branch-delete', target: ref.shortName })}>{t('common.delete')}</DropdownMenuItem>
                </DropdownMenuContent>
              </DropdownMenu>
            </RepositoryRow>
          ))}
        </RepositorySection>
        <RepositorySection icon={<Tags className="size-3.5" />} title={t('sourceControl.tags')}>
          {tags.map((ref) => (
            <RepositoryRow key={ref.fullName} primary={ref.shortName} secondary={(ref.peeledOid ?? ref.targetOid).slice(0, 8)}>
              <DropdownMenu>
                <DropdownMenuTrigger asChild><Button size="icon-xs" variant="ghost" disabled={busy}><Ellipsis className="size-3.5" /></Button></DropdownMenuTrigger>
                <DropdownMenuContent align="end">
                  <DropdownMenuItem disabled={!defaultRemote} onSelect={() => openAction({ kind: 'push-tag', target: ref.shortName })}>{t('sourceControl.pushTag')}</DropdownMenuItem>
                  <DropdownMenuSeparator />
                  <DropdownMenuItem variant="destructive" onSelect={() => openAction({ kind: 'tag-delete', target: ref.shortName })}>{t('common.delete')}</DropdownMenuItem>
                </DropdownMenuContent>
              </DropdownMenu>
            </RepositoryRow>
          ))}
        </RepositorySection>
        <RepositorySection icon={<FileCode2 className="size-3.5" />} title={t('sourceControl.worktrees')}>
          {snapshot.worktrees.map((worktree) => <RepositoryRow key={worktree.path} primary={worktree.branch?.replace('refs/heads/', '') ?? t('sourceControl.detached')} secondary={worktree.path} active={worktree.path === snapshot.repository.workspacePath} />)}
        </RepositorySection>
        <RepositorySection icon={<Archive className="size-3.5" />} title={t('sourceControl.stashes')}>
          {snapshot.stashes.map((stash) => (
            <RepositoryRow key={stash.oid} primary={stash.message} secondary={stash.refName}>
              <Button size="xs" variant="ghost" disabled={busy || locked} onClick={() => openAction({ kind: 'stash-apply', target: stash.refName })}>{t('sourceControl.apply')}</Button>
            </RepositoryRow>
          ))}
        </RepositorySection>
      </ScrollArea>
      <RepositoryActionDialog
        action={action}
        name={name}
        target={target}
        message={message}
        path={path}
        flag={flag}
        style={style}
        remote={remote}
        pullStrategy={pullStrategy}
        remotes={remoteNames}
        onName={setName}
        onTarget={setTarget}
        onMessage={setMessage}
        onPath={setPath}
        onFlag={setFlag}
        onStyle={setStyle}
        onRemote={selectRemote}
        onPullStrategy={setPullStrategy}
        onClose={() => setAction(null)}
        onSubmit={submit}
        valid={valid}
      />
    </div>
  );
}

function RepositoryActionDialog({ action, name, target, message, path, flag, style, remote, pullStrategy, remotes, onName, onTarget, onMessage, onPath, onFlag, onStyle, onRemote, onPullStrategy, onClose, onSubmit, valid }: {
  action: RepositoryAction | null;
  name: string;
  target: string;
  message: string;
  path: string;
  flag: boolean;
  style: 'annotated' | 'lightweight';
  remote: string;
  pullStrategy: GitPullStrategyVm;
  remotes: string[];
  onName: (value: string) => void;
  onTarget: (value: string) => void;
  onMessage: (value: string) => void;
  onPath: (value: string) => void;
  onFlag: (value: boolean) => void;
  onStyle: (value: 'annotated' | 'lightweight') => void;
  onRemote: (value: string) => void;
  onPullStrategy: (value: GitPullStrategyVm) => void;
  onClose: () => void;
  onSubmit: () => void;
  valid: boolean;
}) {
  const { t } = useTranslation();
  if (!action) return null;
  const destructive = action.kind === 'branch-delete' || action.kind === 'tag-delete';
  const showRemote = ['fetch', 'push', 'push-tag'].includes(action.kind);
  const title = t(`sourceControl.actions.${action.kind}.title`);
  return (
    <Dialog open onOpenChange={(open) => { if (!open) onClose(); }}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader><DialogTitle className="text-base">{title}</DialogTitle><DialogDescription>{t(`sourceControl.actions.${action.kind}.description`, { target: action.target ?? '' })}</DialogDescription></DialogHeader>
        <div className="grid gap-3">
          {showRemote ? <Field label={t('sourceControl.remote')}><Select value={remote} onValueChange={onRemote}><SelectTrigger className="w-full"><SelectValue /></SelectTrigger><SelectContent>{remotes.map((item) => <SelectItem key={item} value={item}>{item}</SelectItem>)}</SelectContent></Select></Field> : null}
          {action.kind === 'branch-create' || action.kind === 'branch-rename' || action.kind === 'tag-create' ? <Field label={t('sourceControl.name')}><Input value={name} onChange={(event) => onName(event.target.value)} autoFocus /></Field> : null}
          {action.kind === 'branch-create' || action.kind === 'tag-create' || action.kind === 'worktree-create' ? <Field label={t('sourceControl.startPoint')}><Input value={target} onChange={(event) => onTarget(event.target.value)} /></Field> : null}
          {action.kind === 'worktree-create' ? <><Field label={t('sourceControl.worktreePath')}><Input value={path} onChange={(event) => onPath(event.target.value)} autoFocus /></Field><Field label={t('sourceControl.newBranchOptional')}><Input value={name} onChange={(event) => onName(event.target.value)} /></Field></> : null}
          {action.kind === 'tag-create' ? <Field label={t('sourceControl.tagStyle')}><Select value={style} onValueChange={(value) => onStyle(value as typeof style)}><SelectTrigger className="w-full"><SelectValue /></SelectTrigger><SelectContent><SelectItem value="annotated">{t('sourceControl.annotated')}</SelectItem><SelectItem value="lightweight">{t('sourceControl.lightweight')}</SelectItem></SelectContent></Select></Field> : null}
          {action.kind === 'tag-create' && style === 'annotated' || action.kind === 'stash-create' ? <Field label={t('sourceControl.messageOptional')}><Input value={message} onChange={(event) => onMessage(event.target.value)} /></Field> : null}
          {action.kind === 'pull' ? <Field label={t('sourceControl.pullStrategy')}><Select value={pullStrategy} onValueChange={(value) => onPullStrategy(value as GitPullStrategyVm)}><SelectTrigger className="w-full"><SelectValue /></SelectTrigger><SelectContent><SelectItem value="fast-forward-only">{t('sourceControl.ffOnly')}</SelectItem><SelectItem value="merge">{t('sourceControl.merge')}</SelectItem><SelectItem value="rebase">{t('sourceControl.rebase')}</SelectItem></SelectContent></Select></Field> : null}
          {action.kind === 'fetch' ? <ToggleRow label={t('sourceControl.prune')} checked={flag} onChecked={onFlag} /> : null}
          {action.kind === 'branch-create' ? <ToggleRow label={t('sourceControl.checkoutAfterCreate')} checked={flag} onChecked={onFlag} /> : null}
          {action.kind === 'stash-create' ? <ToggleRow label={t('sourceControl.includeUntracked')} checked={flag} onChecked={onFlag} /> : null}
          {action.kind === 'stash-apply' ? <ToggleRow label={t('sourceControl.restoreIndex')} checked={flag} onChecked={onFlag} /> : null}
        </div>
        <DialogFooter><Button variant="outline" onClick={onClose}>{t('common.cancel')}</Button><Button variant={destructive ? 'destructive' : 'default'} disabled={!valid} onClick={onSubmit}>{destructive ? t('common.delete') : t('common.confirm')}</Button></DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function Field({ label, children }: { label: string; children: ReactNode }) {
  return <div className="grid gap-1.5"><Label>{label}</Label>{children}</div>;
}

function ToggleRow({ label, checked, onChecked }: { label: string; checked: boolean; onChecked: (checked: boolean) => void }) {
  return <div className="flex items-center justify-between gap-3 rounded-md border border-border/60 px-3 py-2"><Label>{label}</Label><Switch checked={checked} onCheckedChange={onChecked} /></div>;
}

function RepositorySection({ icon, title, children }: { icon: ReactNode; title: string; children: ReactNode }) {
  return <section className="py-1"><div className="flex h-8 items-center gap-2 px-3 text-xs font-medium text-muted-foreground">{icon}{title}</div><div>{children}</div><Separator className="mt-1" /></section>;
}

function RepositoryRow({ primary, secondary, active = false, children }: { primary: string; secondary: string; active?: boolean; children?: ReactNode }) {
  return <div className="flex min-w-0 items-center gap-2 px-3 py-1 text-xs"><span className={active ? 'size-2 shrink-0 rounded-full bg-primary' : 'size-2 shrink-0'} /><span className="min-w-0 flex-1 truncate">{primary}</span><span className="max-w-[38%] truncate font-mono text-[10px] text-muted-foreground">{secondary}</span>{children}</div>;
}
