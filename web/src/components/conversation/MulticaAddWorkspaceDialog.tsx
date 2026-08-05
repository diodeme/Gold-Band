import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { FolderInput, Loader2 } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import {
  addMulticaWorkspace,
  listServerMulticaWorkspaces,
  pickLocalDirectory,
} from '../../api';
import { displayAppError } from '../../i18n';
import type { MulticaServerWorkspaceVm } from '../../types';

/// multica 接入 provider 选项（绑定后不可变；与 agent registry agentType 对齐）。
const MULTICA_PROVIDER_OPTIONS = [
  { value: 'claude-acp', label: 'Claude' },
  { value: 'codex-acp', label: 'Codex' },
] as const;

interface MulticaAddWorkspaceDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /// 已绑定的工作空间 id（下拉里过滤掉，避免重复绑定）。
  boundWorkspaceIds: string[];
  /// 添加成功后的回调（调用方刷新远程任务列表）。
  onAdded: () => void;
}

/// 远程任务列表「添加工作空间」弹窗：远程工作空间下拉 + provider 下拉 + 绑定本地目录 + 添加。
///
/// 形态对齐本地任务列表的添加入口（可见、可选项明确），文件目录选择经 `pickLocalDirectory`
/// 做成显式按钮（不再藏在 addMulticaWorkspace 内部黑盒）。仅新 UI（会话侧栏）。
export function MulticaAddWorkspaceDialog({
  open,
  onOpenChange,
  boundWorkspaceIds,
  onAdded,
}: MulticaAddWorkspaceDialogProps) {
  const { t } = useTranslation();
  const [serverWorkspaces, setServerWorkspaces] = useState<MulticaServerWorkspaceVm[]>([]);
  const [workspaceId, setWorkspaceId] = useState('');
  const [provider, setProvider] = useState<string>(MULTICA_PROVIDER_OPTIONS[0].value);
  const [localPath, setLocalPath] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // 每次打开：重置表单 + 拉取服务端工作空间列表供下拉。
  useEffect(() => {
    if (!open) return;
    setWorkspaceId('');
    setProvider(MULTICA_PROVIDER_OPTIONS[0].value);
    setLocalPath(null);
    setError(null);
    let cancelled = false;
    setLoading(true);
    listServerMulticaWorkspaces()
      .then((list) => { if (!cancelled) setServerWorkspaces(list); })
      .catch((err) => { if (!cancelled) setError(displayAppError(t, err)); })
      .finally(() => { if (!cancelled) setLoading(false); });
    return () => { cancelled = true; };
  }, [open, t]);

  async function handlePickDirectory() {
    const path = await pickLocalDirectory();
    if (path) setLocalPath(path);
  }

  async function handleAdd() {
    if (!workspaceId) {
      setError(t('conversation.sidebar.multica.dialog.needWorkspace'));
      return;
    }
    if (!localPath) {
      setError(t('conversation.sidebar.multica.dialog.needDirectory'));
      return;
    }
    const target = serverWorkspaces.find((ws) => ws.id === workspaceId);
    if (!target) return;
    setSubmitting(true);
    setError(null);
    try {
      await addMulticaWorkspace(target.id, target.name, provider, localPath);
      onAdded();
      onOpenChange(false);
    } catch (err) {
      setError(displayAppError(t, err));
    } finally {
      setSubmitting(false);
    }
  }

  const available = serverWorkspaces.filter((ws) => !boundWorkspaceIds.includes(ws.id));

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="flex max-h-[85vh] max-w-md flex-col overflow-hidden gap-0 p-0">
        <DialogHeader className="shrink-0 p-6 pb-0">
          <DialogTitle>{t('conversation.sidebar.multica.dialog.title')}</DialogTitle>
        </DialogHeader>

        <div className="min-h-0 flex-1 space-y-3 overflow-y-auto p-6">
          <div className="space-y-1">
            <div className="text-xs font-medium text-muted-foreground">
              {t('conversation.sidebar.multica.dialog.remoteWorkspace')}
            </div>
            <Select value={workspaceId} onValueChange={setWorkspaceId} disabled={loading || submitting}>
              <SelectTrigger className="h-9 min-w-0 text-xs">
                <SelectValue placeholder={loading ? '…' : t('conversation.sidebar.multica.dialog.remoteWorkspace')} />
              </SelectTrigger>
              <SelectContent>
                {available.map((ws) => (
                  <SelectItem key={ws.id} value={ws.id} className="text-xs">{ws.name}</SelectItem>
                ))}
              </SelectContent>
            </Select>
            {!loading && available.length === 0 && (
              <p className="text-[11px] leading-relaxed text-muted-foreground">
                {serverWorkspaces.length === 0
                  ? t('conversation.sidebar.multica.dialog.noServerWorkspaces')
                  : t('conversation.sidebar.multica.dialog.allWorkspacesBound')}
              </p>
            )}
          </div>

          <div className="space-y-1">
            <div className="text-xs font-medium text-muted-foreground">
              {t('conversation.sidebar.multica.dialog.provider')}
            </div>
            <Select value={provider} onValueChange={setProvider} disabled={submitting}>
              <SelectTrigger className="h-9 min-w-0 font-mono text-xs"><SelectValue /></SelectTrigger>
              <SelectContent>
                {MULTICA_PROVIDER_OPTIONS.map((opt) => (
                  <SelectItem key={opt.value} value={opt.value} className="font-mono text-xs">{opt.label}</SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>

          <div className="space-y-1">
            <div className="text-xs font-medium text-muted-foreground">
              {t('conversation.sidebar.multica.dialog.bindDirectory')}
            </div>
            <div className="flex items-center gap-2">
              <Button type="button" size="sm" variant="outline" className="shrink-0" disabled={submitting} onClick={() => void handlePickDirectory()}>
                <FolderInput className="mr-1.5 size-3.5" />
                {localPath
                  ? t('conversation.sidebar.multica.dialog.changeDirectory')
                  : t('conversation.sidebar.multica.dialog.bindDirectory')}
              </Button>
              <span className="min-w-0 flex-1 truncate font-mono text-[11px] text-muted-foreground">
                {localPath ?? t('conversation.sidebar.multica.dialog.directoryPlaceholder')}
              </span>
            </div>
          </div>

          {error && <p className="text-xs text-destructive">{error}</p>}
        </div>

        <DialogFooter className="shrink-0 border-t border-border/60 p-6 pt-4">
          <Button
            type="button"
            size="sm"
            disabled={submitting || !workspaceId || !localPath}
            onClick={() => void handleAdd()}
          >
            {submitting ? <Loader2 className="mr-1.5 size-3.5 animate-spin" /> : null}
            {t('conversation.sidebar.multica.dialog.add')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
