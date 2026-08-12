import { useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import CodeMirror, { basicSetup, type ReactCodeMirrorRef } from '@uiw/react-codemirror';
import { EditorState, type Extension } from '@codemirror/state';
import { EditorView, lineNumbers } from '@codemirror/view';
import { getChunks, goToNextChunk, goToPreviousChunk, unifiedMergeView } from '@codemirror/merge';
import { ChevronDown, ChevronUp, FileDiff, FileText, TriangleAlert } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { getFileComparison, getGitComparison } from '@/api';
import { Button } from '@/components/ui/button';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import type { FileComparisonVm, GitFileComparisonVm } from '@/types';
import type { GitFileComparisonWorkspaceResource, TurnFileWorkspaceResource } from '../right-workspace-context';
import { WorkspaceFileEditor } from './WorkspaceFileEditor';
import {
  loadWorkspaceLanguageForPath,
  workspaceEditorTheme,
  workspaceSyntaxHighlighting,
} from './editor-extensions';
import { isMarkdownDocumentPath } from './markdown-document';
import type { MarkdownEditorMode } from './file-content-store';

type FileComparisonWorkspaceResource = TurnFileWorkspaceResource | GitFileComparisonWorkspaceResource;
type WorkspaceComparisonVm = FileComparisonVm | GitFileComparisonVm;

export function TurnFileWorkspacePanel({ resource }: { resource: FileComparisonWorkspaceResource }) {
  const { t } = useTranslation();
  const editorRef = useRef<ReactCodeMirrorRef>(null);
  const [comparison, setComparison] = useState<WorkspaceComparisonVm | null>(null);
  const [language, setLanguage] = useState<Extension | null>(null);
  const [errorCode, setErrorCode] = useState<string | null>(null);
  const [markdownMode, setMarkdownMode] = useState<MarkdownEditorMode>('live-preview');
  const [diffChunkCount, setDiffChunkCount] = useState(0);

  useEffect(() => {
    let cancelled = false;
    setComparison(null);
    setErrorCode(null);
    setDiffChunkCount(0);
    const request = 'gitSource' in resource
      ? getGitComparison(resource.projectId, resource.gitSource)
      : getFileComparison(resource.locator, resource.changeSetId, resource.changeId);
    void request
      .then((next) => { if (!cancelled) setComparison(next); })
      .catch((reason: unknown) => {
        if (cancelled) return;
        setErrorCode(typeof reason === 'object' && reason && 'code' in reason && typeof reason.code === 'string'
          ? reason.code
          : 'turn-files.change-set-not-found');
      });
    return () => { cancelled = true; };
  }, [resource]);

  useEffect(() => {
    let cancelled = false;
    if (isMarkdownDocumentPath(resource.title)) {
      setLanguage(null);
      return () => { cancelled = true; };
    }
    void loadWorkspaceLanguageForPath(resource.title).then((extension) => {
      if (!cancelled) setLanguage(extension);
    });
    return () => { cancelled = true; };
  }, [resource.title]);

  useEffect(() => setMarkdownMode('live-preview'), [resource.key]);

  const before = comparison?.before?.content ?? '';
  const after = comparison?.after?.content ?? '';
  const markdownVersion = resource.kind === 'file-version'
    && isMarkdownDocumentPath(comparison?.path ?? resource.title);
  const showDiffChunkNavigation = shouldShowDiffChunkNavigation(diffChunkCount);
  const extensions = useMemo(() => {
    const base: Extension[] = [
      basicSetup({ lineNumbers: false, foldGutter: false, drawSelection: false }),
      lineNumbers(),
      EditorState.readOnly.of(true),
      EditorView.editable.of(false),
      EditorView.lineWrapping,
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
    <section className="flex min-h-0 min-w-0 max-w-full flex-1 flex-col" data-turn-file-workspace={resource.kind}>
      <header className="z-10 shrink-0 border-b border-border/60 bg-background/95 backdrop-blur">
        <div className="flex h-9 items-center gap-2 px-3 text-xs">
          {resource.kind === 'file-diff' ? <FileDiff className="size-3.5 text-foreground" /> : <FileText className="size-3.5 text-foreground" />}
          <span className="min-w-0 flex-1 truncate font-mono text-foreground">{comparison.path}</span>
          <span className="tabular-nums text-emerald-600 dark:text-emerald-400">+{comparison.stats.addedLines ?? 0}</span>
          <span className="tabular-nums text-destructive">-{comparison.stats.deletedLines ?? 0}</span>
        </div>
        {resource.kind === 'file-diff' ? (
          <div className="flex h-9 items-center gap-1 border-t border-border/40 px-2">
            <span className="mr-auto px-1 text-xs text-muted-foreground">{'gitSource' in resource ? t('sourceControl.diff') : t('turnFiles.turnDiff')}</span>
            {showDiffChunkNavigation ? <>
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
            </> : null}
          </div>
        ) : null}
      </header>
      <div className="min-h-0 min-w-0 max-w-full flex-1 overflow-hidden">
        {markdownVersion ? (
          <WorkspaceFileEditor
            documentKey={resource.key}
            value={after}
            editable={false}
            language="markdown"
            highlight
            contentRevision={0}
            target={null}
            targetRevision={0}
            onChange={() => undefined}
            onSave={() => undefined}
            initialStateJson={null}
            onPersistState={() => undefined}
            markdownMode={markdownMode}
            onMarkdownModeChange={setMarkdownMode}
          />
        ) : (
          <CodeMirror
            ref={editorRef}
            value={after}
            height="100%"
            width="100%"
            theme="none"
            basicSetup={false}
            editable={false}
            extensions={extensions}
            onCreateEditor={(view) => setDiffChunkCount(getChunks(view.state)?.chunks.length ?? 0)}
            className="h-full min-h-0 min-w-0 max-w-full overflow-hidden [&_.cm-editor]:h-full [&_.cm-editor]:max-w-full [&_.cm-scroller]:max-w-full [&_.cm-scroller]:overflow-y-auto [&_.cm-scroller]:overflow-x-hidden"
            aria-label={resource.kind === 'file-diff' ? t('turnFiles.diffViewer') : t('turnFiles.versionViewer')}
          />
        )}
      </div>
    </section>
  );
}

export function shouldShowDiffChunkNavigation(chunkCount: number) {
  return chunkCount > 1;
}

function PanelMessage({ icon, text }: { icon?: ReactNode; text: string }) {
  return <div className="flex min-h-0 flex-1 items-center justify-center gap-2 px-6 text-center text-sm text-muted-foreground">{icon}{text}</div>;
}
