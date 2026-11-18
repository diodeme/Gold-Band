import { useEffect, useRef, useState, type ReactElement } from 'react';
import { useTranslation } from 'react-i18next';
import { ContextMenu, ContextMenuContent, ContextMenuTrigger } from '@/components/ui/context-menu';
import { Popover, PopoverAnchor, PopoverContent } from '@/components/ui/popover';
import { useReadOnlyExperience } from '@/components/ReadOnlyExperience';
import { displayAppError } from '@/i18n';
import { resolveRightWorkspaceFileTarget, revealRightWorkspaceFile } from '@/api';
import type { PromptWorkspaceFileRef, RightWorkspaceFileTargetVm } from '@/types';
import { fileReferenceIdentity, rightWorkspaceFileSource } from '@/lib/file-reference-source';
import { useRightWorkspaceCommands, type RightWorkspaceResource } from './right-workspace-context';
import { useWorkspaceFileReferenceCommands, useWorkspaceFileReferencePresentation } from './workspace-file-reference-bridge';
import { WorkspaceFileActionItems } from './files/WorkspaceFileActionItems';

type TargetState = { kind: 'idle' | 'loading' } | { kind: 'ready'; value: RightWorkspaceFileTargetVm } | { kind: 'error'; message: string };

export function RightWorkspaceFileMenu({ tab, children }: { tab: RightWorkspaceResource; children: ReactElement }) {
  const source = rightWorkspaceFileSource(tab);
  return source ? <FileMenu key={fileReferenceIdentity(source)} tabKey={tab.key} source={source}>{children}</FileMenu> : children;
}

function FileMenu({ tabKey, source, children }: { tabKey: string; source: PromptWorkspaceFileRef; children: ReactElement }) {
  const { t } = useTranslation();
  const readOnly = useReadOnlyExperience();
  const commands = useRightWorkspaceCommands();
  const composer = useWorkspaceFileReferenceCommands();
  const { isDocked } = useWorkspaceFileReferencePresentation();
  const [state, setState] = useState<TargetState>({ kind: 'idle' });
  const [failure, setFailure] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const pendingAction = useRef(false);
  const request = useRef(0);
  const mounted = useRef(true);
  const sourceKey = fileReferenceIdentity(source);
  const currentKey = useRef(sourceKey);
  currentKey.current = sourceKey;
  useEffect(() => { mounted.current = true; return () => { mounted.current = false; request.current++; }; }, []);
  const stillCurrent = () => {
    const current = commands.getResource(tabKey);
    const currentSource = current && rightWorkspaceFileSource(current);
    return mounted.current && currentKey.current === sourceKey && currentSource !== null
      && fileReferenceIdentity(currentSource) === sourceKey;
  };
  const openChanged = (open: boolean) => {
    const revision = ++request.current;
    if (!open) return;
    setFailure(null);
    setState({ kind: 'loading' });
    void resolveRightWorkspaceFileTarget(source).then(value => {
      if (revision === request.current && stillCurrent()) setState({ kind: 'ready', value });
    }, error => {
      if (revision === request.current && stillCurrent()) setState({ kind: 'error', message: displayAppError(t, error) });
    });
  };
  const run = (reference: boolean) => {
    if (pendingAction.current || !stillCurrent()) return;
    const addReference = reference ? composer?.captureFileReferenceTarget() : null;
    if (reference && !addReference) return;
    pendingAction.current = true;
    setBusy(true);
    void (async () => {
      if (!reference) { await revealRightWorkspaceFile(source); return; }
      const target = await resolveRightWorkspaceFileTarget(source);
      if (!stillCurrent()) return;
      if (target.availability.kind !== 'available') throw target.availability;
      const result = addReference!({ id: sourceKey, source, projectId: target.projectId,
        relativePath: target.relativePath ?? target.name, name: target.name, byteLength: target.byteLength,
        mimeType: target.mimeType, ...(target.absolutePath ? { canonicalPath: target.absolutePath } : {}) }, { isDocked });
      if (result.kind === 'unavailable') return;
    })().catch(error => { if (stillCurrent()) setFailure(displayAppError(t, error)); })
      .finally(() => { pendingAction.current = false; if (mounted.current) setBusy(false); });
  };
  const target = state.kind === 'ready' ? state.value : null;
  const unavailable = target?.availability.kind === 'unavailable' ? displayAppError(t, target.availability) : null;
  return <Popover open={failure !== null || busy} onOpenChange={open => { if (!open) setFailure(null); }}>
    <ContextMenu onOpenChange={openChanged}>
      <PopoverAnchor asChild><ContextMenuTrigger asChild>{children}</ContextMenuTrigger></PopoverAnchor>
      <ContextMenuContent className="min-w-48" data-right-workspace-file-menu={tabKey}>
        <WorkspaceFileActionItems absolutePath={target?.absolutePath ?? null} relativePath={target?.relativePath ?? null}
          relativeBaseLabel={target ? t(target.relativeBase === 'attempt'
            ? 'workspace.filesPanel.relativeToRunDirectory'
            : target.relativeBase === 'attachment'
              ? 'workspace.filesPanel.relativeToAttachmentDirectory'
              : 'workspace.filesPanel.relativeToSourceWorkspace') : undefined}
          onCopyFailed={() => { if (stillCurrent()) setFailure(t('workspace.filesPanel.pathCopyFailed')); }}
          reveal={{ disabled: readOnly || busy || target?.availability.kind !== 'available', run: () => run(false) }}
          reference={{ disabled: readOnly || busy || !composer?.available || target?.availability.kind !== 'available', run: () => run(true) }} />
        {state.kind === 'loading' || state.kind === 'error' || unavailable ? <p role="status" className="max-w-64 px-2 py-1 text-xs text-muted-foreground">
          {state.kind === 'loading' ? t('workspace.filesPanel.loadingFile') : state.kind === 'error' ? state.message : unavailable}
        </p> : null}
      </ContextMenuContent>
    </ContextMenu>
    <PopoverContent className="max-w-72 p-2 text-xs" role={busy ? 'status' : 'alert'} side="bottom"
      onOpenAutoFocus={event => event.preventDefault()} onCloseAutoFocus={event => event.preventDefault()}>
      <span className={failure ? 'text-destructive' : 'text-muted-foreground'}>{busy ? t('workspace.filesPanel.loadingFile') : failure}</span>
    </PopoverContent>
  </Popover>;
}
