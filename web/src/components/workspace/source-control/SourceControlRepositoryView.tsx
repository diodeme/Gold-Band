import { useMemo, useState, type ReactNode } from 'react';
import { Ellipsis, LoaderCircle, Plus } from 'lucide-react';
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
import { Switch } from '@/components/ui/switch';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs';
import type {
  GitMutationRequestVm,
  GitOperationRequestVm,
  GitSourceControlSnapshotVm,
} from '@/types';
import { rememberPreferredGitRemote, resolvePreferredGitRemote } from './source-control-preferences';
import type { SourceControlRepositoryTab } from './source-control-store';

type RepositoryActionKind =
  | 'branch-create'
  | 'branch-rename'
  | 'branch-delete'
  | 'tag-create'
  | 'tag-delete'
  | 'push-tag'
  | 'worktree-create'
  | 'worktree-remove'
  | 'stash-apply';

type RepositoryAction = { kind: RepositoryActionKind; target?: string };

export function SourceControlRepositoryView({
  snapshot,
  busyActionKind,
  busyActionPath,
  locked,
  onMutation,
  onOperation,
  activeTab,
  onTabChange,
}: {
  snapshot: GitSourceControlSnapshotVm;
  busyActionKind: string | null;
  busyActionPath: string | null;
  locked: boolean;
  onMutation: (input: GitMutationRequestVm) => void;
  onOperation: (input: GitOperationRequestVm) => void;
  activeTab: SourceControlRepositoryTab;
  onTabChange: (tab: SourceControlRepositoryTab) => void;
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
  };

  const selectRemote = (value: string) => {
    setRemote(value);
    rememberPreferredGitRemote(snapshot.repository.commonDir, value);
  };

  const submit = () => {
    if (!action) return;
    switch (action.kind) {
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
      case 'worktree-remove':
        onMutation({ kind: 'worktree-remove', path: action.target ?? '' });
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
      case 'branch-create':
      case 'branch-rename':
      case 'tag-create': return name.trim().length > 0;
      case 'worktree-create': return path.trim().length > 0;
      case 'push-tag': return remote.length > 0;
      default: return true;
    }
  }, [action, name, path, remote]);

  return (
    <div className="flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden" data-source-control-repository="true">
      <Tabs value={activeTab} onValueChange={(value) => onTabChange(value as SourceControlRepositoryTab)} className="min-h-0 min-w-0 flex-1 gap-0 overflow-hidden">
      <div className="flex h-9 shrink-0 items-center border-b border-border/50 px-2">
        <TabsList variant="line" className="h-9 min-w-0 flex-1 justify-start">
          <TabsTrigger value="branches" className="text-xs">{t('sourceControl.branches')}</TabsTrigger>
          <TabsTrigger value="tags" className="text-xs">{t('sourceControl.tags')}</TabsTrigger>
          <TabsTrigger value="worktrees" className="text-xs">{t('sourceControl.worktrees')}</TabsTrigger>
          <TabsTrigger value="stashes" className="text-xs">{t('sourceControl.stashes')}</TabsTrigger>
        </TabsList>
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button className="ml-auto" size="icon-xs" variant="ghost" disabled={busy} aria-label={t('sourceControl.repositoryActions')}><Plus className="size-3.5" /></Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end">
            <DropdownMenuItem onSelect={() => openAction({ kind: 'branch-create' })}>{t('sourceControl.createBranch')}</DropdownMenuItem>
            <DropdownMenuItem onSelect={() => openAction({ kind: 'tag-create' })}>{t('sourceControl.createTag')}</DropdownMenuItem>
            <DropdownMenuItem onSelect={() => openAction({ kind: 'worktree-create' })}>{t('sourceControl.createWorktree')}</DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      </div>
      <TabsContent value="branches" className="min-h-0 min-w-0 overflow-hidden data-[state=active]:flex data-[state=active]:flex-1"><ScrollArea className="min-h-0 min-w-0 flex-1">
        <div className="py-1">
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
        </div>
      </ScrollArea></TabsContent>
      <TabsContent value="tags" className="min-h-0 min-w-0 overflow-hidden data-[state=active]:flex data-[state=active]:flex-1"><ScrollArea className="min-h-0 min-w-0 flex-1"><div className="min-w-0 py-1">
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
        </div></ScrollArea></TabsContent>
      <TabsContent value="worktrees" className="min-h-0 min-w-0 overflow-hidden data-[state=active]:flex data-[state=active]:flex-1"><ScrollArea className="min-h-0 min-w-0 flex-1"><div className="min-w-0 py-1">
          {snapshot.worktrees.map((worktree) => {
            const current = worktree.path === snapshot.repository.workspacePath;
            const removing = busyActionKind === 'worktree-remove' && busyActionPath === worktree.path;
            return (
              <RepositoryRow key={worktree.path} primary={worktree.branch?.replace('refs/heads/', '') ?? t('sourceControl.detached')} secondary={worktree.path} active={current}>
                {removing ? <LoaderCircle className="size-3.5 shrink-0 animate-spin text-muted-foreground" aria-label={t('sourceControl.removingWorktree')} /> : (
                  <DropdownMenu>
                    <DropdownMenuTrigger asChild><Button size="icon-xs" variant="ghost" disabled={busy} aria-label={t('sourceControl.worktreeActions', { path: worktree.path })}><Ellipsis className="size-3.5" /></Button></DropdownMenuTrigger>
                    <DropdownMenuContent align="end">
                      <DropdownMenuItem variant="destructive" disabled={current || locked} onSelect={() => openAction({ kind: 'worktree-remove', target: worktree.path })}>{t('sourceControl.removeWorktree')}</DropdownMenuItem>
                    </DropdownMenuContent>
                  </DropdownMenu>
                )}
              </RepositoryRow>
            );
          })}
        </div></ScrollArea></TabsContent>
      <TabsContent value="stashes" className="min-h-0 min-w-0 overflow-hidden data-[state=active]:flex data-[state=active]:flex-1"><ScrollArea className="min-h-0 min-w-0 flex-1"><div className="min-w-0 py-1">
          {snapshot.stashes.map((stash) => (
            <RepositoryRow key={stash.oid} primary={stash.message} secondary={stash.refName}>
              <Button size="xs" variant="ghost" disabled={busy || locked} onClick={() => openAction({ kind: 'stash-apply', target: stash.refName })}>{t('sourceControl.apply')}</Button>
            </RepositoryRow>
          ))}
        </div></ScrollArea></TabsContent>
      </Tabs>
      <RepositoryActionDialog
        action={action}
        name={name}
        target={target}
        message={message}
        path={path}
        flag={flag}
        style={style}
        remote={remote}
        remotes={remoteNames}
        onName={setName}
        onTarget={setTarget}
        onMessage={setMessage}
        onPath={setPath}
        onFlag={setFlag}
        onStyle={setStyle}
        onRemote={selectRemote}
        onClose={() => setAction(null)}
        onSubmit={submit}
        valid={valid}
      />
    </div>
  );
}

function RepositoryActionDialog({ action, name, target, message, path, flag, style, remote, remotes, onName, onTarget, onMessage, onPath, onFlag, onStyle, onRemote, onClose, onSubmit, valid }: {
  action: RepositoryAction | null;
  name: string;
  target: string;
  message: string;
  path: string;
  flag: boolean;
  style: 'annotated' | 'lightweight';
  remote: string;
  remotes: string[];
  onName: (value: string) => void;
  onTarget: (value: string) => void;
  onMessage: (value: string) => void;
  onPath: (value: string) => void;
  onFlag: (value: boolean) => void;
  onStyle: (value: 'annotated' | 'lightweight') => void;
  onRemote: (value: string) => void;
  onClose: () => void;
  onSubmit: () => void;
  valid: boolean;
}) {
  const { t } = useTranslation();
  if (!action) return null;
  const destructive = action.kind === 'branch-delete' || action.kind === 'tag-delete' || action.kind === 'worktree-remove';
  const showRemote = action.kind === 'push-tag';
  const title = t(`sourceControl.actions.${action.kind}.title`);
  return (
    <Dialog open onOpenChange={(open) => { if (!open) onClose(); }}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader><DialogTitle className="text-base">{title}</DialogTitle><DialogDescription>{t(`sourceControl.actions.${action.kind}.description`, { target: action.target ?? '' })}</DialogDescription></DialogHeader>
        <div className="grid gap-3">
          {action.kind === 'worktree-remove' ? <div className="min-w-0 break-all rounded-md bg-muted/50 px-3 py-2 font-mono text-xs text-muted-foreground">{action.target}</div> : null}
          {showRemote ? <Field label={t('sourceControl.remote')}><Select value={remote} onValueChange={onRemote}><SelectTrigger className="w-full"><SelectValue /></SelectTrigger><SelectContent>{remotes.map((item) => <SelectItem key={item} value={item}>{item}</SelectItem>)}</SelectContent></Select></Field> : null}
          {action.kind === 'branch-create' || action.kind === 'branch-rename' || action.kind === 'tag-create' ? <Field label={t('sourceControl.name')}><Input value={name} onChange={(event) => onName(event.target.value)} autoFocus /></Field> : null}
          {action.kind === 'branch-create' || action.kind === 'tag-create' || action.kind === 'worktree-create' ? <Field label={t('sourceControl.startPoint')}><Input value={target} onChange={(event) => onTarget(event.target.value)} /></Field> : null}
          {action.kind === 'worktree-create' ? <><Field label={t('sourceControl.worktreePath')}><Input value={path} onChange={(event) => onPath(event.target.value)} autoFocus /></Field><Field label={t('sourceControl.newBranchOptional')}><Input value={name} onChange={(event) => onName(event.target.value)} /></Field></> : null}
          {action.kind === 'tag-create' ? <Field label={t('sourceControl.tagStyle')}><Select value={style} onValueChange={(value) => onStyle(value as typeof style)}><SelectTrigger className="w-full"><SelectValue /></SelectTrigger><SelectContent><SelectItem value="annotated">{t('sourceControl.annotated')}</SelectItem><SelectItem value="lightweight">{t('sourceControl.lightweight')}</SelectItem></SelectContent></Select></Field> : null}
          {action.kind === 'tag-create' && style === 'annotated' ? <Field label={t('sourceControl.messageOptional')}><Input value={message} onChange={(event) => onMessage(event.target.value)} /></Field> : null}
          {action.kind === 'branch-create' ? <ToggleRow label={t('sourceControl.checkoutAfterCreate')} checked={flag} onChecked={onFlag} /> : null}
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

function RepositoryRow({ primary, secondary, active = false, children }: { primary: string; secondary: string; active?: boolean; children?: ReactNode }) {
  return <div className="flex w-full min-w-0 max-w-full items-center gap-2 overflow-hidden px-3 py-1 text-xs"><span className={active ? 'size-2 shrink-0 rounded-full bg-primary' : 'size-2 shrink-0'} /><span className="min-w-0 flex-1 truncate">{primary}</span><span className="min-w-0 max-w-[32%] shrink truncate font-mono text-[10px] text-muted-foreground">{secondary}</span>{children ? <span className="flex shrink-0 items-center">{children}</span> : null}</div>;
}
