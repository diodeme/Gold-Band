import { useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import CodeMirror, { basicSetup, type ReactCodeMirrorRef } from '@uiw/react-codemirror';
import { EditorSelection, EditorState, type Extension } from '@codemirror/state';
import { EditorView, lineNumbers } from '@codemirror/view';
import { getChunks, goToNextChunk, goToPreviousChunk } from '@codemirror/merge';
import { ChevronDown, ChevronLeft, ChevronRight, ChevronUp, FileDiff, FileText, LoaderCircle, TriangleAlert } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { getGitComparison } from '@/api';
import { Button } from '@/components/ui/button';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { loadTurnFileComparison } from '@/lib/turn-file-comparison-cache';
import type { FileComparisonVm, GitFileComparisonVm } from '@/types';
import type { GitFileComparisonWorkspaceResource, TurnFileWorkspaceResource } from '../right-workspace-context';
import { useRightWorkspaceCommands } from '../right-workspace-context';
import { WorkspaceFileEditor } from './WorkspaceFileEditor';
import { githubComparisonCache } from '../source-control/github-comparison-cache';
import { diffReviewStore, resolveDiffReviewNavigation } from '../source-control/diff-review-store';
import {
  loadWorkspaceLanguageForPath,
  workspaceEditorTheme,
  workspaceSyntaxHighlighting,
} from './editor-extensions';
import { isMarkdownDocumentPath } from './markdown-document';
import type { MarkdownEditorMode } from './file-content-store';
import { ReadonlyUnifiedDiff } from './ReadonlyUnifiedDiff';

export { DIFF_VIEW_SCAN_LIMIT, DIFF_VIEW_TIMEOUT_MS } from './ReadonlyUnifiedDiff';

type FileComparisonWorkspaceResource = TurnFileWorkspaceResource | GitFileComparisonWorkspaceResource;
type WorkspaceComparisonVm = FileComparisonVm | GitFileComparisonVm;

export function TurnFileWorkspacePanel({ resource }: { resource: FileComparisonWorkspaceResource }) {
  const { t } = useTranslation();
  const editorRef = useRef<ReactCodeMirrorRef>(null);
  const rightWorkspace = useRightWorkspaceCommands();
  const [comparison, setComparison] = useState<WorkspaceComparisonVm | null>(null);
  const [language, setLanguage] = useState<Extension | null>(null);
  const [errorCode, setErrorCode] = useState<string | null>(null);
  const [markdownMode, setMarkdownMode] = useState<MarkdownEditorMode>('live-preview');
  const [diffChunkCount, setDiffChunkCount] = useState(0);
  const [activeChunkIndex, setActiveChunkIndex] = useState(0);

  const gitResource = 'gitSource' in resource ? resource : null;
  const reviewSession = gitResource?.reviewSessionId
    ? diffReviewStore.get(gitResource.reviewSessionId)
    : null;
  const reviewItemIndex = reviewSession && gitResource?.reviewItemId
    ? reviewSession.items.findIndex((item) => item.id === gitResource.reviewItemId)
    : -1;
  const reviewItem = reviewItemIndex >= 0 ? reviewSession?.items[reviewItemIndex] ?? null : null;

  useEffect(() => {
    let cancelled = false;
    setComparison(null);
    setErrorCode(null);
    setDiffChunkCount(0);
    const request = 'gitSource' in resource
      ? resource.reviewSessionId && resource.reviewItemId
        ? reviewItem
          ? diffReviewStore.comparison(resource.projectId, reviewItem)
          : Promise.reject({ code: 'git.diff-review-session-expired' })
        : resource.gitSource.kind === 'github-pr'
        ? githubComparisonCache.get(resource.projectId, resource.gitSource)
        : getGitComparison(resource.projectId, resource.gitSource)
      : loadTurnFileComparison(resource.locator, resource.changeSetId, resource.changeId);
    void request
      .then((next) => {
        if (cancelled) return;
        setComparison(next);
        if ('gitSource' in resource && resource.reviewSessionId && resource.reviewItemId) {
          diffReviewStore.prefetchAdjacent(resource.reviewSessionId, resource.reviewItemId);
        }
      })
      .catch((reason: unknown) => {
        if (cancelled) return;
        setErrorCode(typeof reason === 'object' && reason && 'code' in reason && typeof reason.code === 'string'
          ? reason.code
          : 'turn-files.change-set-not-found');
      });
    return () => { cancelled = true; };
  }, [resource, reviewItem]);

  const navigateReviewFile = (offset: number, landing: 'top' | 'first-change' | 'last-change') => {
    if (!('gitSource' in resource) || !reviewSession || reviewItemIndex < 0) return;
    const item = reviewSession.items[reviewItemIndex + offset];
    if (!item) return;
    void rightWorkspace.openResource({
      ...resource,
      title: item.path.split('/').at(-1) ?? item.path,
      description: item.path,
      gitSource: item.source,
      reviewItemId: item.id,
      reviewLanding: landing,
    });
  };

  const focusChunk = (index: number) => {
    const view = editorRef.current?.view;
    const chunks = view ? getChunks(view.state)?.chunks ?? [] : [];
    const chunk = chunks[index];
    if (!view || !chunk) return false;
    const from = Math.min(chunk.fromB, view.state.doc.length);
    const to = Math.min(chunk.toB, view.state.doc.length);
    const range = EditorSelection.range(to, from);
    view.dispatch({
      selection: range,
      effects: EditorView.scrollIntoView(range),
    });
    setActiveChunkIndex(index);
    return true;
  };

  const navigateReviewChange = (direction: -1 | 1) => {
    if (!reviewSession) {
      const view = editorRef.current?.view;
      if (view) (direction < 0 ? goToPreviousChunk : goToNextChunk)(view);
      return;
    }
    const target = resolveDiffReviewNavigation({
      itemIndex: reviewItemIndex,
      itemCount: reviewSession.items.length,
      chunkIndex: activeChunkIndex,
      chunkCount: diffChunkCount,
      direction,
    });
    if (target.kind === 'chunk') focusChunk(target.index);
    if (target.kind === 'file') navigateReviewFile(
      target.offset,
      target.landing === 'first' ? 'first-change' : 'last-change',
    );
  };

  useEffect(() => {
    let cancelled = false;
    if (resource.kind === 'file-diff' || isMarkdownDocumentPath(resource.title)) {
      setLanguage(null);
      return () => { cancelled = true; };
    }
    void loadWorkspaceLanguageForPath(resource.title).then((extension) => {
      if (!cancelled) setLanguage(extension);
    });
    return () => { cancelled = true; };
  }, [resource.title]);

  useEffect(() => setMarkdownMode('live-preview'), [resource.key]);

  const after = comparison?.after?.content ?? '';
  const markdownVersion = resource.kind === 'file-version'
    && isMarkdownDocumentPath(comparison?.path ?? resource.title);
  const showDiffChunkNavigation = reviewSession ? diffChunkCount > 0 || reviewSession.items.length > 1 : shouldShowDiffChunkNavigation(diffChunkCount);
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
    return base;
  }, [language]);

  if (errorCode) {
    return <PanelMessage icon={<TriangleAlert className="size-4 text-destructive" />} text={t(`errors.${errorCode}`, { defaultValue: t('turnFiles.loadFailed') })} />;
  }
  if (!comparison) {
    return <PanelMessage icon={<LoaderCircle className="size-4 animate-spin" />} text={t('turnFiles.loading')} />;
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
            <span className="mr-auto px-1 text-xs text-muted-foreground">
              {reviewSession ? `${reviewItemIndex + 1} / ${reviewSession.items.length}` : 'gitSource' in resource ? t('sourceControl.diff') : t('turnFiles.turnDiff')}
            </span>
            {reviewSession ? <>
              <Tooltip><TooltipTrigger asChild><Button size="icon" variant="ghost" className="size-7" disabled={reviewItemIndex <= 0} aria-label={t('sourceControl.previousFile')} onClick={() => navigateReviewFile(-1, 'top')}><ChevronLeft className="size-3.5" /></Button></TooltipTrigger><TooltipContent>{t('sourceControl.previousFile')}</TooltipContent></Tooltip>
              <Tooltip><TooltipTrigger asChild><Button size="icon" variant="ghost" className="size-7" disabled={reviewItemIndex >= reviewSession.items.length - 1} aria-label={t('sourceControl.nextFile')} onClick={() => navigateReviewFile(1, 'top')}><ChevronRight className="size-3.5" /></Button></TooltipTrigger><TooltipContent>{t('sourceControl.nextFile')}</TooltipContent></Tooltip>
            </> : null}
            {showDiffChunkNavigation ? <>
              <Tooltip>
                <TooltipTrigger asChild>
                  <Button size="icon" variant="ghost" className="size-7" aria-label={t('turnFiles.previousChange')} onClick={() => navigateReviewChange(-1)}>
                    <ChevronUp className="size-3.5" />
                  </Button>
                </TooltipTrigger>
                <TooltipContent>{t('turnFiles.previousChange')}</TooltipContent>
              </Tooltip>
              <Tooltip>
                <TooltipTrigger asChild>
                  <Button size="icon" variant="ghost" className="size-7" aria-label={t('turnFiles.nextChange')} onClick={() => navigateReviewChange(1)}>
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
        ) : resource.kind === 'file-diff' ? (
          <ReadonlyUnifiedDiff
            comparison={comparison}
            editorRef={editorRef}
            ariaLabel={t('turnFiles.diffViewer')}
            onCreateEditor={(view) => {
              const count = getChunks(view.state)?.chunks.length ?? 0;
              setDiffChunkCount(count);
              const initialIndex = 'gitSource' in resource && resource.reviewLanding === 'last-change'
                ? Math.max(0, count - 1)
                : 0;
              setActiveChunkIndex(initialIndex);
              const shouldFocusChunk = count > 0
                && 'gitSource' in resource
                && resource.reviewSessionId
                && (resource.reviewLanding === 'first-change' || resource.reviewLanding === 'last-change');
              if (shouldFocusChunk) {
                requestAnimationFrame(() => focusChunk(initialIndex));
              }
            }}
          />
        ) : (
          <CodeMirror
            value={after}
            height="100%"
            width="100%"
            theme="none"
            basicSetup={false}
            editable={false}
            extensions={extensions}
            className="h-full min-h-0 min-w-0 max-w-full overflow-hidden [&_.cm-editor]:h-full [&_.cm-editor]:max-w-full [&_.cm-scroller]:max-w-full [&_.cm-scroller]:overflow-y-auto [&_.cm-scroller]:overflow-x-hidden"
            aria-label={t('turnFiles.versionViewer')}
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
