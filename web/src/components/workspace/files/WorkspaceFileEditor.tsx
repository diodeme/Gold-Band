import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import CodeMirror, { basicSetup, type ReactCodeMirrorRef } from '@uiw/react-codemirror';
import { historyField } from '@codemirror/commands';
import { foldGutter, HighlightStyle, syntaxHighlighting } from '@codemirror/language';
import { highlightSelectionMatches } from '@codemirror/search';
import { Compartment, EditorSelection, EditorState, Prec, type Extension } from '@codemirror/state';
import {
  EditorView,
  highlightActiveLine,
  highlightActiveLineGutter,
  keymap,
  lineNumbers,
} from '@codemirror/view';
import { tags } from '@lezer/highlight';
import { Check, Code2, Copy, Eye } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Button } from '@/components/ui/button';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { isDocumentAnchorHref } from '@/lib/file-link';
import type { FileTargetLocationVm } from '@/types';
import type { MarkdownEditorMode, MarkdownImageState } from './file-content-store';
import { loadMarkdownLivePreviewExtensions } from './markdown-live-preview';
import {
  markdownImagePreview,
  predecodeMarkdownImagesNearViewport,
  updateMarkdownImagePreview,
} from './markdown-image-preview';
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

interface PendingMarkdownModeTransition {
  mode: MarkdownEditorMode;
  viewport: EditorViewportAnchor;
  targetRevisionAtCapture: number;
}

interface MarkdownExtensionProfile {
  revision: number;
  extensions: Extension[];
}

const EMPTY_MARKDOWN_IMAGES = new Map<string, MarkdownImageState>();

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

export function restoreEditorViewportAnchor(view: EditorView, anchor: EditorViewportAnchor) {
  view.dispatch({
    effects: editorViewportScrollEffect(view, anchor),
  });
}

export function editorViewportScrollEffect(view: EditorView, anchor: EditorViewportAnchor) {
  const position = Math.min(view.state.doc.length, Math.max(0, anchor.position));
  return EditorView.scrollIntoView(position, {
    y: 'start',
    yMargin: Math.max(0, anchor.blockOffsetTop),
  });
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
  const onMarkdownImagePreviewErrorRef = useRef(onMarkdownImagePreviewError);
  const onLocationAdjustedRef = useRef(onLocationAdjusted);
  const onPersistStateRef = useRef(onPersistState);
  const valueRef = useRef(value);
  const targetRevisionRef = useRef(targetRevision);
  const markdownImagesRef = useRef(markdownImages);
  const pendingModeTransitionRef = useRef<PendingMarkdownModeTransition | null>(null);
  const modeTransitionRequestRef = useRef(0);
  const appliedTargetRevisionsRef = useRef(new Map<string, number>());
  const appliedModeProfileRef = useRef<string | null>(null);
  const initialModeProfileRef = useRef<string | null>(null);
  const editorExtensionsRef = useRef<Extension[] | null>(null);
  const targetIntentRef = useRef({ documentKey, target, targetRevision });
  const modeCompartment = useMemo(() => new Compartment(), []);
  const editorPolicyCompartment = useMemo(() => new Compartment(), []);
  const [languageExtension, setLanguageExtension] = useState<Extension | null>(null);
  const [languageExtensionRevision, setLanguageExtensionRevision] = useState(0);
  const [markdownProfile, setMarkdownProfile] = useState<MarkdownExtensionProfile | null>(null);
  const [activeEditorView, setActiveEditorView] = useState<EditorView | null>(null);
  const [modeTransitionPending, setModeTransitionPending] = useState(false);
  const [copied, setCopied] = useState(false);
  const copiedTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  onSaveRef.current = onSave;
  onChangeRef.current = onChange;
  onMarkdownLinkClickRef.current = onMarkdownLinkClick;
  onMarkdownImagePreviewErrorRef.current = onMarkdownImagePreviewError;
  onLocationAdjustedRef.current = onLocationAdjusted;
  onPersistStateRef.current = onPersistState;
  markdownImagesRef.current = markdownImages;
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
  const routeMarkdownImagePreviewError = useCallback((rawSrc: string, failedToken: string) => {
    onMarkdownImagePreviewErrorRef.current?.(rawSrc, failedToken);
  }, []);

  useEffect(() => {
    let active = true;
    if (!highlight) {
      setLanguageExtension(null);
      setLanguageExtensionRevision((revision) => revision + 1);
      return () => { active = false; };
    }
    void loadLanguage(language).then((extension) => {
      if (active) {
        setLanguageExtension(extension);
        setLanguageExtensionRevision((revision) => revision + 1);
      }
    });
    return () => { active = false; };
  }, [highlight, language]);

  useEffect(() => {
    let active = true;
    if (markdownMode === null || !markdownLivePreviewAvailable) {
      setMarkdownProfile(null);
      return () => { active = false; };
    }
    void loadMarkdownLivePreviewExtensions(routeMarkdownLink, !markdownHasTableImages).then((extensions) => {
      if (active) {
        setMarkdownProfile((current) => ({
          revision: (current?.revision ?? 0) + 1,
          extensions,
        }));
      }
    });
    return () => { active = false; };
  }, [markdownHasTableImages, markdownLivePreviewAvailable, markdownMode === null, routeMarkdownLink]);

  const previewMode = markdownMode === 'live-preview' && markdownLivePreviewAvailable;
  const desiredEditorMode: MarkdownEditorMode = previewMode && markdownProfile ? 'live-preview' : 'source';
  const editorReady = editorExtensionsRef.current !== null
    || markdownMode === null
    || !previewMode
    || markdownProfile !== null;
  const baseEditorExtensions = useMemo<Extension[]>(() => [
    ...basicSetup({
      lineNumbers: false,
      highlightActiveLineGutter: false,
      foldGutter: false,
      highlightActiveLine: false,
      highlightSelectionMatches: false,
      searchKeymap: true,
    }),
    workspaceEditorTheme,
    syntaxHighlighting(workspaceHighlightStyle),
    EditorView.lineWrapping,
    Prec.highest(keymap.of([{ key: 'Mod-s', preventDefault: true, run: () => { onSaveRef.current(); return true; } }])),
  ], []);
  const sourceModeExtensions = useMemo<Extension[]>(() => [
    ...(languageExtension ? [languageExtension] : []),
    lineNumbers(),
    highlightActiveLineGutter(),
    highlightActiveLine(),
    ...(highlight ? [foldGutter(), highlightSelectionMatches()] : []),
  ], [highlight, languageExtension]);
  const modeProfileSignature = desiredEditorMode === 'live-preview'
    ? `live-preview:${markdownProfile?.revision ?? 0}:${highlight ? 1 : 0}`
    : `source:${languageExtensionRevision}:${highlight ? 1 : 0}`;
  const currentModeExtensions = useCallback((): Extension[] => {
    if (desiredEditorMode === 'live-preview' && markdownProfile) {
      return [
        ...markdownProfile.extensions,
        markdownImagePreview(
          markdownImagesRef.current,
          routeMarkdownImagePreviewError,
          routeMarkdownLink,
        ),
        ...(highlight ? [highlightSelectionMatches()] : []),
      ];
    }
    return sourceModeExtensions;
  }, [desiredEditorMode, highlight, markdownProfile, routeMarkdownImagePreviewError, routeMarkdownLink, sourceModeExtensions]);
  if (editorReady && !editorExtensionsRef.current) {
    editorExtensionsRef.current = [
      ...baseEditorExtensions,
      editorPolicyCompartment.of([
        EditorState.readOnly.of(!editable),
        EditorView.editable.of(editable),
      ]),
      modeCompartment.of(currentModeExtensions()),
    ];
    initialModeProfileRef.current = modeProfileSignature;
  }
  const handleChange = useCallback((nextValue: string) => {
    if (nextValue !== valueRef.current) onChangeRef.current(nextValue);
  }, []);

  const applyPendingTarget = useCallback((view: EditorView) => {
    const intent = targetIntentRef.current;
    const appliedTargetRevision = appliedTargetRevisionsRef.current.get(intent.documentKey) ?? 0;
    if (
      !intent.target?.line
      || intent.targetRevision <= appliedTargetRevision
    ) return false;
    const resolved = targetSelection(view, intent.target);
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
    const view = activeEditorView;
    if (!view || editorRef.current?.view !== view) return;
    view.dispatch({
      effects: editorPolicyCompartment.reconfigure([
        EditorState.readOnly.of(!editable),
        EditorView.editable.of(editable),
      ]),
    });
  }, [activeEditorView, editable, editorPolicyCompartment]);

  useEffect(() => {
    const view = activeEditorView;
    if (
      !view
      || editorRef.current?.view !== view
      || appliedModeProfileRef.current === modeProfileSignature
    ) return;
    const pendingTransition = pendingModeTransitionRef.current;
    const viewport = pendingTransition?.mode === desiredEditorMode
      ? pendingTransition.viewport
      : captureEditorViewportAnchor(view);
    const hasNewerTarget = Boolean(
      pendingTransition
      && target?.line
      && targetRevision > pendingTransition.targetRevisionAtCapture,
    );
    view.dispatch({
      effects: [
        modeCompartment.reconfigure(currentModeExtensions()),
        ...(!hasNewerTarget ? [editorViewportScrollEffect(view, viewport)] : []),
      ],
    });
    appliedModeProfileRef.current = modeProfileSignature;
    if (pendingTransition?.mode === desiredEditorMode) {
      pendingModeTransitionRef.current = null;
      setModeTransitionPending(false);
    }
  }, [
    activeEditorView,
    currentModeExtensions,
    desiredEditorMode,
    modeCompartment,
    modeProfileSignature,
    target?.line,
    targetRevision,
  ]);

  useEffect(() => {
    if (desiredEditorMode !== 'live-preview') return;
    const view = activeEditorView;
    if (view && editorRef.current?.view === view) {
      updateMarkdownImagePreview(
        view,
        markdownImages,
        routeMarkdownImagePreviewError,
        routeMarkdownLink,
      );
    }
  }, [
    activeEditorView,
    desiredEditorMode,
    markdownImages,
    modeProfileSignature,
    routeMarkdownImagePreviewError,
    routeMarkdownLink,
  ]);

  useEffect(() => {
    if (!target?.line) return;
    if (activeEditorView && editorRef.current?.view === activeEditorView) applyPendingTarget(activeEditorView);
  }, [activeEditorView, applyPendingTarget, contentRevision, documentKey, modeProfileSignature, target, targetRevision]);

  useEffect(() => () => {
    modeTransitionRequestRef.current += 1;
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

  const switchMarkdownMode = async () => {
    if (!markdownMode || !onMarkdownModeChange || modeTransitionPending) return;
    const nextMode = previewMode ? 'source' : 'live-preview';
    const view = editorRef.current?.view;
    if (!view || (nextMode === 'live-preview' && !markdownProfile)) return;
    const request = modeTransitionRequestRef.current + 1;
    modeTransitionRequestRef.current = request;
    setModeTransitionPending(true);
    if (nextMode === 'live-preview') {
      await predecodeMarkdownImagesNearViewport(view, markdownImagesRef.current);
      if (modeTransitionRequestRef.current !== request) return;
    }
    pendingModeTransitionRef.current = {
      mode: nextMode,
      viewport: captureEditorViewportAnchor(view),
      targetRevisionAtCapture: targetRevisionRef.current,
    };
    onMarkdownModeChange(nextMode);
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
                disabled={modeTransitionPending || (!previewMode && (!markdownLivePreviewAvailable || !markdownProfile))}
                onClick={() => void switchMarkdownMode()}
                aria-label={t(previewMode ? 'workspace.filesPanel.viewMarkdownSource' : 'workspace.filesPanel.viewMarkdownLivePreview')}
              >
                {previewMode ? <Code2 className="size-3" /> : <Eye className="size-3" />}
              </Button>
            </TooltipTrigger>
            <TooltipContent>{t(previewMode ? 'workspace.filesPanel.viewMarkdownSource' : 'workspace.filesPanel.viewMarkdownLivePreview')}</TooltipContent>
          </Tooltip>
        </div>
      ) : null}
      {editorReady ? (
        <CodeMirror
          ref={editorRef}
          value={editorValue}
          height="100%"
          theme="none"
          basicSetup={false}
          extensions={editorExtensionsRef.current ?? []}
          initialState={initialStateJson ? { json: initialStateJson, fields: { history: historyField } } : undefined}
          onCreateEditor={(view) => {
            appliedModeProfileRef.current = initialModeProfileRef.current;
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
