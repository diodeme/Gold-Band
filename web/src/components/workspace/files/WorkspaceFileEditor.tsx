import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import CodeMirror, { type ReactCodeMirrorRef } from '@uiw/react-codemirror';
import { historyField } from '@codemirror/commands';
import { HighlightStyle, syntaxHighlighting } from '@codemirror/language';
import { EditorSelection, EditorState, Prec, type Extension } from '@codemirror/state';
import { EditorView, keymap } from '@codemirror/view';
import { tags } from '@lezer/highlight';
import { Check, Code2, Copy, Eye } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Button } from '@/components/ui/button';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { isDocumentAnchorHref } from '@/lib/file-link';
import type { FileTargetLocationVm } from '@/types';
import type { MarkdownEditorMode, MarkdownImageState } from './file-content-store';
import { loadMarkdownLivePreviewExtensions } from './markdown-live-preview';
import { markdownImagePreview, updateMarkdownImagePreview } from './markdown-image-preview';
import '@atomic-editor/editor/styles.css';

interface WorkspaceFileEditorProps {
  documentKey: string;
  value: string;
  editable: boolean;
  language: string | null;
  highlight: boolean;
  contentRevision: number;
  target: FileTargetLocationVm | null;
  targetRevision: number;
  onChange: (value: string) => void;
  onSave: () => void;
  initialStateJson: unknown | null;
  onPersistState: (state: unknown) => void;
  onLocationAdjusted?: (adjusted: boolean) => void;
  markdownMode?: MarkdownEditorMode | null;
  markdownLivePreviewAvailable?: boolean;
  onMarkdownModeChange?: (mode: MarkdownEditorMode) => void;
  markdownImages?: ReadonlyMap<string, MarkdownImageState>;
  markdownHasTableImages?: boolean;
  onMarkdownImagePreviewError?: (rawSrc: string, failedToken: string) => void;
  onMarkdownLinkClick?: (href: string) => void;
}

interface EditorViewportAnchor {
  position: number;
  blockOffsetTop: number;
}

interface PendingEditorRebuild {
  mode: MarkdownEditorMode;
  stateJson: unknown;
  viewport: EditorViewportAnchor;
  targetRevisionAtCapture: number;
}

const EMPTY_MARKDOWN_IMAGES = new Map<string, MarkdownImageState>();
const VIEWPORT_RESTORE_PASSES = 3;
const VIEWPORT_RESTORE_EPSILON_PX = 0.5;

const workspaceEditorTheme = EditorView.theme({
  '&': { height: '100%', backgroundColor: 'transparent', color: 'var(--foreground)' },
  '.cm-scroller': { fontFamily: 'var(--font-mono, ui-monospace)', fontSize: '12px', lineHeight: '1.6' },
  '.cm-content': { padding: '12px 0' },
  '.cm-gutters': { backgroundColor: 'transparent', color: 'var(--muted-foreground)', borderRight: '1px solid color-mix(in srgb, var(--border) 55%, transparent)' },
  '.cm-activeLine, .cm-activeLineGutter': { backgroundColor: 'color-mix(in srgb, var(--muted) 35%, transparent)' },
  '.cm-selectionBackground, &.cm-focused .cm-selectionBackground': { backgroundColor: 'color-mix(in srgb, var(--primary) 20%, transparent)' },
  '&.cm-focused': { outline: 'none' },
});

const workspaceHighlightStyle = HighlightStyle.define([
  { tag: [tags.comment, tags.lineComment, tags.blockComment, tags.docComment], color: 'var(--muted-foreground)', fontStyle: 'italic' },
  { tag: [tags.meta, tags.processingInstruction, tags.punctuation], color: 'var(--muted-foreground)' },
  { tag: [tags.keyword, tags.controlKeyword, tags.operatorKeyword, tags.modifier], color: 'var(--gold-running)' },
  { tag: [tags.function(tags.variableName), tags.function(tags.propertyName), tags.labelName], color: 'var(--gold-running)' },
  { tag: [tags.string, tags.special(tags.string), tags.regexp, tags.escape], color: 'var(--gold-success)' },
  { tag: [tags.number, tags.bool, tags.null, tags.atom], color: 'var(--gold-warning)' },
  { tag: [tags.invalid, tags.deleted], color: 'var(--gold-danger)' },
  { tag: [tags.heading, tags.strong], color: 'var(--foreground)', fontWeight: '600' },
  { tag: tags.emphasis, fontStyle: 'italic' },
  { tag: [tags.link, tags.url], color: 'var(--gold-running)', textDecoration: 'underline' },
]);

async function loadLanguage(language: string | null): Promise<Extension | null> {
  if (!language) return null;
  const { languages } = await import('@codemirror/language-data');
  const normalized = language.toLowerCase();
  const description = languages.find((candidate) => (
    candidate.name.toLowerCase() === normalized
    || candidate.alias.some((alias) => alias.toLowerCase() === normalized)
  ));
  return description ? description.load() : null;
}

export function captureEditorViewportAnchor(view: EditorView): EditorViewportAnchor {
  const viewportTop = Math.max(0, view.scrollDOM.getBoundingClientRect().top - view.documentTop);
  const block = view.lineBlockAtHeight(viewportTop);
  return {
    position: Math.min(view.state.doc.length, block.from),
    blockOffsetTop: block.top - viewportTop,
  };
}

function viewportAnchorDocumentTop(view: EditorView, anchor: EditorViewportAnchor) {
  const position = Math.min(view.state.doc.length, Math.max(0, anchor.position));
  const block = view.lineBlockAt(position);
  return block.top - anchor.blockOffsetTop;
}

function serializeEditorState(view: EditorView) {
  return view.state.toJSON({ history: historyField });
}

export function normalizeCodeMirrorValue(value: string) {
  return value.replace(/\r\n?/gu, '\n');
}

function targetSelection(view: EditorView, target: FileTargetLocationVm) {
  const lineNumber = Math.min(Math.max(1, target.line ?? 1), view.state.doc.lines);
  const line = view.state.doc.line(lineNumber);
  const columnOffset = Math.min(Math.max(0, (target.column ?? 1) - 1), line.length);
  const anchor = line.from + columnOffset;
  const head = target.endLine && target.endLine >= lineNumber
    ? view.state.doc.line(Math.min(target.endLine, view.state.doc.lines)).to
    : anchor;
  return {
    selection: EditorSelection.single(anchor, head),
    anchor,
    adjusted: target.line !== lineNumber
      || (target.column != null && target.column - 1 !== columnOffset)
      || (target.endLine != null && target.endLine > view.state.doc.lines),
  };
}

export function WorkspaceFileEditor({
  documentKey,
  value,
  editable,
  language,
  highlight,
  contentRevision,
  target,
  targetRevision,
  onChange,
  onSave,
  initialStateJson,
  onPersistState,
  onLocationAdjusted,
  markdownMode = null,
  markdownLivePreviewAvailable = true,
  onMarkdownModeChange,
  markdownImages = EMPTY_MARKDOWN_IMAGES,
  markdownHasTableImages = false,
  onMarkdownImagePreviewError,
  onMarkdownLinkClick,
}: WorkspaceFileEditorProps) {
  const { t } = useTranslation();
  const editorRef = useRef<ReactCodeMirrorRef>(null);
  const onSaveRef = useRef(onSave);
  const onChangeRef = useRef(onChange);
  const onMarkdownLinkClickRef = useRef(onMarkdownLinkClick);
  const onLocationAdjustedRef = useRef(onLocationAdjusted);
  const onPersistStateRef = useRef(onPersistState);
  const valueRef = useRef(value);
  const targetRevisionRef = useRef(targetRevision);
  const pendingRebuildRef = useRef<PendingEditorRebuild | null>(null);
  const appliedTargetRevisionsRef = useRef(new Map<string, number>());
  const targetIntentRef = useRef({ documentKey, target, targetRevision });
  const [languageExtension, setLanguageExtension] = useState<Extension | null>(null);
  const [markdownExtensions, setMarkdownExtensions] = useState<Extension[]>([]);
  const [markdownExtensionsReady, setMarkdownExtensionsReady] = useState(markdownMode === null);
  const [markdownProfileRevision, setMarkdownProfileRevision] = useState(0);
  const [activeEditorView, setActiveEditorView] = useState<EditorView | null>(null);
  const [copied, setCopied] = useState(false);
  const copiedTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  onSaveRef.current = onSave;
  onChangeRef.current = onChange;
  onMarkdownLinkClickRef.current = onMarkdownLinkClick;
  onLocationAdjustedRef.current = onLocationAdjusted;
  onPersistStateRef.current = onPersistState;
  const editorValue = useMemo(() => normalizeCodeMirrorValue(value), [value]);
  valueRef.current = editorValue;
  targetRevisionRef.current = targetRevision;
  targetIntentRef.current = { documentKey, target, targetRevision };
  const routeMarkdownLink = useCallback((href: string) => {
    if (isDocumentAnchorHref(href)) {
      if (href.trim() === '#') editorRef.current?.view?.scrollDOM.scrollTo({ left: 0, top: 0 });
      return;
    }
    onMarkdownLinkClickRef.current?.(href);
  }, []);

  useEffect(() => {
    let active = true;
    if (!highlight) {
      setLanguageExtension(null);
      return () => { active = false; };
    }
    void loadLanguage(language).then((extension) => {
      if (active) setLanguageExtension(extension);
    });
    return () => { active = false; };
  }, [highlight, language]);

  const captureForRebuild = useCallback((mode: MarkdownEditorMode) => {
    const view = editorRef.current?.view;
    if (!view) return;
    pendingRebuildRef.current = {
      mode,
      stateJson: serializeEditorState(view),
      viewport: captureEditorViewportAnchor(view),
      targetRevisionAtCapture: targetRevisionRef.current,
    };
    onPersistStateRef.current(pendingRebuildRef.current.stateJson);
  }, []);

  useEffect(() => {
    let active = true;
    if (markdownMode === null || !markdownLivePreviewAvailable) {
      setMarkdownExtensions([]);
      setMarkdownExtensionsReady(true);
      return () => { active = false; };
    }
    if (markdownMode === 'live-preview') captureForRebuild('live-preview');
    setMarkdownExtensionsReady(false);
    void loadMarkdownLivePreviewExtensions(routeMarkdownLink, !markdownHasTableImages).then((extensions) => {
      if (active) {
        setMarkdownExtensions(extensions);
        setMarkdownExtensionsReady(true);
        setMarkdownProfileRevision((revision) => revision + 1);
      }
    });
    return () => { active = false; };
  }, [captureForRebuild, markdownHasTableImages, markdownLivePreviewAvailable, markdownMode === null, routeMarkdownLink]);

  const previewMode = markdownMode === 'live-preview' && markdownLivePreviewAvailable;
  const activeEditorReady = !previewMode || markdownExtensionsReady;
  const markdownImageExtension = useMemo(() => markdownImagePreview(), []);

  const sharedExtensions = useMemo<Extension[]>(() => [
    workspaceEditorTheme,
    syntaxHighlighting(workspaceHighlightStyle),
    EditorView.lineWrapping,
    EditorState.readOnly.of(!editable),
    EditorView.editable.of(editable),
    Prec.highest(keymap.of([{ key: 'Mod-s', preventDefault: true, run: () => { onSaveRef.current(); return true; } }])),
  ], [editable]);
  const sourceExtensions = useMemo<Extension[]>(() => [
    ...sharedExtensions,
    ...(languageExtension ? [languageExtension] : []),
  ], [languageExtension, sharedExtensions]);
  const previewExtensions = useMemo<Extension[]>(() => [
    ...sharedExtensions,
    ...markdownExtensions,
    markdownImageExtension,
  ], [markdownExtensions, markdownImageExtension, sharedExtensions]);
  const activeExtensions = previewMode ? previewExtensions : sourceExtensions;
  const basicSetup = useMemo(() => ({
    lineNumbers: !previewMode,
    foldGutter: highlight && !previewMode,
    highlightActiveLine: !previewMode,
    highlightSelectionMatches: highlight,
    searchKeymap: true,
  }), [highlight, previewMode]);
  const handleChange = useCallback((nextValue: string) => {
    if (nextValue !== valueRef.current) onChangeRef.current(nextValue);
  }, []);
  const editorProfileKey = previewMode ? `preview-${markdownProfileRevision}` : 'source';
  const pendingRebuild = pendingRebuildRef.current?.mode === markdownMode ? pendingRebuildRef.current : null;
  const editorInitialState = pendingRebuild?.stateJson ?? initialStateJson;

  const applyPendingTarget = useCallback((view: EditorView) => {
    const intent = targetIntentRef.current;
    const appliedTargetRevision = appliedTargetRevisionsRef.current.get(intent.documentKey) ?? 0;
    if (
      !intent.target?.line
      || intent.targetRevision <= appliedTargetRevision
    ) return false;
    const resolved = targetSelection(view, intent.target);
    const pendingTargetRebuild = pendingRebuildRef.current;
    if (pendingTargetRebuild && intent.targetRevision > pendingTargetRebuild.targetRevisionAtCapture) {
      pendingRebuildRef.current = null;
    }
    view.dispatch({
      selection: resolved.selection,
      effects: EditorView.scrollIntoView(resolved.selection.main, { y: 'center' }),
    });
    appliedTargetRevisionsRef.current.set(intent.documentKey, intent.targetRevision);
    onLocationAdjustedRef.current?.(resolved.adjusted);
    view.focus();
    return true;
  }, []);

  useEffect(() => {
    if (!previewMode || !activeEditorReady) return;
    const view = activeEditorView;
    if (view) updateMarkdownImagePreview(view, markdownImages, onMarkdownImagePreviewError, routeMarkdownLink);
  }, [activeEditorReady, activeEditorView, markdownImages, onMarkdownImagePreviewError, previewMode, editorProfileKey, routeMarkdownLink]);

  useEffect(() => {
    if (!activeEditorReady || !target?.line) return;
    if (activeEditorView && editorRef.current?.view === activeEditorView) applyPendingTarget(activeEditorView);
  }, [activeEditorReady, activeEditorView, applyPendingTarget, contentRevision, documentKey, editorProfileKey, target, targetRevision]);

  useEffect(() => {
    const viewport = pendingRebuild?.viewport;
    if (
      !activeEditorReady
      || !viewport
      || (target?.line && targetRevision > pendingRebuild.targetRevisionAtCapture)
    ) return;
    let cancelled = false;
    let frame = 0;
    let pass = 0;
    const restore = () => {
      const view = editorRef.current?.view;
      if (!view || cancelled) return;
      view.requestMeasure({
        read: () => {
          const currentTop = view.scrollDOM.getBoundingClientRect().top - view.documentTop;
          return { currentTop, targetTop: viewportAnchorDocumentTop(view, viewport) };
        },
        write: ({ currentTop, targetTop }) => {
          if (cancelled) return;
          const delta = targetTop - currentTop;
          if (Math.abs(delta) > VIEWPORT_RESTORE_EPSILON_PX) view.scrollDOM.scrollTop += delta;
          pass += 1;
          if (pass < VIEWPORT_RESTORE_PASSES) {
            frame = requestAnimationFrame(restore);
          } else {
            pendingRebuildRef.current = null;
          }
        },
      });
    };
    frame = requestAnimationFrame(restore);
    return () => {
      cancelled = true;
      cancelAnimationFrame(frame);
    };
  }, [activeEditorReady, documentKey, editorProfileKey, pendingRebuild, target?.line, targetRevision]);

  useEffect(() => () => {
    if (copiedTimerRef.current) clearTimeout(copiedTimerRef.current);
    const state = editorRef.current?.view?.state;
    if (state) onPersistStateRef.current(state.toJSON({ history: historyField }));
  }, []);

  const copySource = async () => {
    const source = editorRef.current?.view?.state.doc.toString() ?? value;
    await navigator.clipboard.writeText(source);
    setCopied(true);
    if (copiedTimerRef.current) clearTimeout(copiedTimerRef.current);
    copiedTimerRef.current = setTimeout(() => setCopied(false), 1_500);
  };

  const switchMarkdownMode = () => {
    if (!markdownMode) return;
    const nextMode = previewMode ? 'source' : 'live-preview';
    captureForRebuild(nextMode);
    onMarkdownModeChange?.(nextMode);
  };

  return (
    <div className="relative h-full min-h-0">
      {markdownMode ? (
        <div className="absolute right-2 top-2 z-20 flex items-center gap-0.5 rounded-md border border-border/50 bg-background/88 p-0.5 shadow-sm backdrop-blur">
          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                size="icon"
                variant="ghost"
                className="size-6"
                onClick={() => void copySource()}
                aria-label={t('workspace.filesPanel.copyMarkdownSource')}
              >
                {copied ? <Check className="size-3 text-emerald-600" /> : <Copy className="size-3" />}
              </Button>
            </TooltipTrigger>
            <TooltipContent>{t(copied ? 'workspace.filesPanel.markdownSourceCopied' : 'workspace.filesPanel.copyMarkdownSource')}</TooltipContent>
          </Tooltip>
          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                size="icon"
                variant="ghost"
                className="size-6"
                disabled={!markdownLivePreviewAvailable && markdownMode === 'source'}
                onClick={switchMarkdownMode}
                aria-label={t(previewMode ? 'workspace.filesPanel.viewMarkdownSource' : 'workspace.filesPanel.viewMarkdownLivePreview')}
              >
                {previewMode ? <Code2 className="size-3" /> : <Eye className="size-3" />}
              </Button>
            </TooltipTrigger>
            <TooltipContent>{t(previewMode ? 'workspace.filesPanel.viewMarkdownSource' : 'workspace.filesPanel.viewMarkdownLivePreview')}</TooltipContent>
          </Tooltip>
        </div>
      ) : null}
      {activeEditorReady ? (
        <CodeMirror
          key={editorProfileKey}
          ref={editorRef}
          value={editorValue}
          height="100%"
          theme="none"
          basicSetup={basicSetup}
          extensions={activeExtensions}
          initialState={editorInitialState ? { json: editorInitialState, fields: { history: historyField } } : undefined}
          onCreateEditor={(view) => {
            if (previewMode) updateMarkdownImagePreview(view, markdownImages, onMarkdownImagePreviewError, routeMarkdownLink);
            setActiveEditorView(view);
          }}
          onChange={handleChange}
          onBlur={() => onSaveRef.current()}
          className={previewMode
            ? 'atomic-cm-editor workspace-markdown-live-preview h-full min-h-0 overflow-hidden [&_.cm-editor]:h-full [&_.cm-scroller]:overflow-auto'
            : 'h-full min-h-0 overflow-hidden [&_.cm-editor]:h-full [&_.cm-scroller]:overflow-auto'}
          aria-label="workspace-file-editor"
        />
      ) : (
        <div className="flex h-full items-center justify-center text-sm text-muted-foreground" aria-label="workspace-markdown-loading">…</div>
      )}
    </div>
  );
}
