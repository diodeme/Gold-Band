import { useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import CodeMirror, { basicSetup, type ReactCodeMirrorRef } from '@uiw/react-codemirror';
import { EditorState, type Extension } from '@codemirror/state';
import { EditorView, lineNumbers } from '@codemirror/view';
import { goToNextChunk, goToPreviousChunk, unifiedMergeView } from '@codemirror/merge';
import { ChevronDown, ChevronUp, FileDiff, FileText, TriangleAlert } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { getFileComparison } from '@/api';
import { Button } from '@/components/ui/button';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import type { FileComparisonVm } from '@/types';
import type { TurnFileWorkspaceResource } from '../right-workspace-context';
import {
  loadWorkspaceLanguageForPath,
  workspaceEditorTheme,
  workspaceSyntaxHighlighting,
} from './editor-extensions';

export function TurnFileWorkspacePanel({ resource }: { resource: TurnFileWorkspaceResource }) {
  const { t } = useTranslation();
  const editorRef = useRef<ReactCodeMirrorRef>(null);
  const [comparison, setComparison] = useState<FileComparisonVm | null>(null);
  const [language, setLanguage] = useState<Extension | null>(null);
  const [errorCode, setErrorCode] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    setComparison(null);
    setErrorCode(null);
    void getFileComparison(resource.locator, resource.changeSetId, resource.changeId)
      .then((next) => { if (!cancelled) setComparison(next); })
      .catch((reason: unknown) => {
        if (cancelled) return;
        setErrorCode(typeof reason === 'object' && reason && 'code' in reason && typeof reason.code === 'string'
          ? reason.code
          : 'turn-files.change-set-not-found');
      });
    return () => { cancelled = true; };
  }, [resource.changeId, resource.changeSetId, resource.locator]);

  useEffect(() => {
    let cancelled = false;
    void loadWorkspaceLanguageForPath(resource.title).then((extension) => {
      if (!cancelled) setLanguage(extension);
    });
    return () => { cancelled = true; };
  }, [resource.title]);

  const before = comparison?.before?.content ?? '';
  const after = comparison?.after?.content ?? '';
  const extensions = useMemo(() => {
    const base: Extension[] = [
      basicSetup({ lineNumbers: false, foldGutter: false }),
      lineNumbers(),
      EditorState.readOnly.of(true),
      EditorView.editable.of(false),
      workspaceEditorTheme,
      workspaceSyntaxHighlighting,
    ];
    if (language) base.push(language);
    if (resource.kind === 'file-diff') {
      base.push(unifiedMergeView({
        original: before,
        highlightChanges: true,
        gutter: true,
        mergeControls: false,
        collapseUnchanged: { margin: 3, minSize: 8 },
      }));
    }
    return base;
  }, [before, language, resource.kind]);

  if (errorCode) {
    return <PanelMessage icon={<TriangleAlert className="size-4 text-destructive" />} text={t(`errors.${errorCode}`, { defaultValue: t('turnFiles.loadFailed') })} />;
  }
  if (!comparison) {
    return <PanelMessage text={t('turnFiles.loading')} />;
  }
  if (comparison.limitationCode && !comparison.after && !comparison.before) {
    return <PanelMessage icon={<TriangleAlert className="size-4 text-amber-500" />} text={t(`errors.${comparison.limitationCode}`, { defaultValue: t('turnFiles.diffUnavailable') })} />;
  }

  return (
    <section className="flex min-h-0 flex-1 flex-col" data-turn-file-workspace={resource.kind}>
      <header className="z-10 shrink-0 border-b border-border/60 bg-background/95 backdrop-blur">
        <div className="flex h-9 items-center gap-2 px-3 text-xs">
          {resource.kind === 'file-diff' ? <FileDiff className="size-3.5 text-primary" /> : <FileText className="size-3.5 text-primary" />}
          <span className="min-w-0 flex-1 truncate font-mono text-foreground">{comparison.path}</span>
          <span className="tabular-nums text-emerald-600 dark:text-emerald-400">+{comparison.stats.addedLines ?? 0}</span>
          <span className="tabular-nums text-destructive">-{comparison.stats.deletedLines ?? 0}</span>
        </div>
        {resource.kind === 'file-diff' ? (
          <div className="flex h-9 items-center gap-1 border-t border-border/40 px-2">
            <span className="mr-auto px-1 text-xs text-muted-foreground">{t('turnFiles.beforeThisTurn')}</span>
            <Tooltip>
              <TooltipTrigger asChild>
                <Button size="icon" variant="ghost" className="size-7" aria-label={t('turnFiles.previousChange')} onClick={() => editorRef.current?.view && goToPreviousChunk(editorRef.current.view)}>
                  <ChevronUp className="size-3.5" />
                </Button>
              </TooltipTrigger>
              <TooltipContent>{t('turnFiles.previousChange')}</TooltipContent>
            </Tooltip>
            <Tooltip>
              <TooltipTrigger asChild>
                <Button size="icon" variant="ghost" className="size-7" aria-label={t('turnFiles.nextChange')} onClick={() => editorRef.current?.view && goToNextChunk(editorRef.current.view)}>
                  <ChevronDown className="size-3.5" />
                </Button>
              </TooltipTrigger>
              <TooltipContent>{t('turnFiles.nextChange')}</TooltipContent>
            </Tooltip>
          </div>
        ) : null}
      </header>
      <div className="min-h-0 flex-1 overflow-hidden">
        <CodeMirror
          ref={editorRef}
          value={after}
          height="100%"
          theme="none"
          basicSetup={false}
          editable={false}
          extensions={extensions}
          className="h-full min-h-0 overflow-hidden [&_.cm-editor]:h-full [&_.cm-scroller]:overflow-auto"
          aria-label={resource.kind === 'file-diff' ? t('turnFiles.diffViewer') : t('turnFiles.versionViewer')}
        />
      </div>
    </section>
  );
}

function PanelMessage({ icon, text }: { icon?: ReactNode; text: string }) {
  return <div className="flex min-h-0 flex-1 items-center justify-center gap-2 px-6 text-center text-sm text-muted-foreground">{icon}{text}</div>;
}
