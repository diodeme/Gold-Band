import { useEffect, useMemo, useRef, useState } from 'react';
import CodeMirror, { type ReactCodeMirrorRef } from '@uiw/react-codemirror';
import { keymap } from '@codemirror/view';
import { historyField } from '@codemirror/commands';
import { EditorSelection, EditorState, Prec } from '@codemirror/state';
import { HighlightStyle, syntaxHighlighting } from '@codemirror/language';
import { EditorView } from '@codemirror/view';
import { tags } from '@lezer/highlight';
import type { Extension } from '@codemirror/state';
import { Check, Code2, Copy, Eye } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Button } from '@/components/ui/button';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import type { FileTargetLocationVm } from '@/types';
import type { MarkdownEditorMode } from './file-content-store';
import type { MarkdownImageState } from './file-content-store';
import { loadMarkdownLivePreviewExtensions } from './markdown-live-preview';
import { markdownImagePreview } from './markdown-image-preview';
import '@atomic-editor/editor/styles.css';

interface WorkspaceFileEditorProps {
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
  { tag: [tags.keyword, tags.controlKeyword, tags.operatorKeyword, tags.modifier], color: 'var(--primary)' },
  { tag: [tags.function(tags.variableName), tags.function(tags.propertyName), tags.labelName], color: 'var(--primary)' },
  { tag: [tags.string, tags.special(tags.string), tags.regexp, tags.escape], color: 'var(--gold-success)' },
  { tag: [tags.number, tags.bool, tags.null, tags.atom], color: 'var(--gold-warning)' },
  { tag: [tags.invalid, tags.deleted], color: 'var(--gold-danger)' },
  { tag: [tags.heading, tags.strong], color: 'var(--foreground)', fontWeight: '600' },
  { tag: tags.emphasis, fontStyle: 'italic' },
  { tag: [tags.link, tags.url], color: 'var(--primary)', textDecoration: 'underline' },
]);

async function loadLanguage(language: string | null): Promise<Extension | null> {
  if (!language) return null;
  const { languages } = await import('@codemirror/language-data');
  const normalized = language.toLowerCase();
  const description = languages.find((candidate) =>
    candidate.name.toLowerCase() === normalized
    || candidate.alias.some((alias) => alias.toLowerCase() === normalized),
  );
  return description ? description.load() : null;
}

export function WorkspaceFileEditor({
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
}: WorkspaceFileEditorProps) {
  const { t } = useTranslation();
  const editorRef = useRef<ReactCodeMirrorRef>(null);
  const sourceEditorRef = useRef<ReactCodeMirrorRef>(null);
  const onSaveRef = useRef(onSave);
  const [languageExtension, setLanguageExtension] = useState<Extension | null>(null);
  const [markdownExtensions, setMarkdownExtensions] = useState<Extension[]>([]);
  const [markdownExtensionsReady, setMarkdownExtensionsReady] = useState(markdownMode === null);
  const [copied, setCopied] = useState(false);
  const copiedTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  onSaveRef.current = onSave;

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

  useEffect(() => {
    let active = true;
    if (markdownMode === null || !markdownLivePreviewAvailable) {
      setMarkdownExtensions([]);
      setMarkdownExtensionsReady(true);
      return () => { active = false; };
    }
    setMarkdownExtensionsReady(false);
    void loadMarkdownLivePreviewExtensions(() => undefined, !markdownHasTableImages).then((extensions) => {
      if (active) {
        setMarkdownExtensions(extensions);
        setMarkdownExtensionsReady(true);
      }
    });
    return () => { active = false; };
  }, [markdownHasTableImages, markdownLivePreviewAvailable, markdownMode === null]);

  const previewMode = markdownMode === 'live-preview' && markdownLivePreviewAvailable;
  const markdownImageExtension = useMemo(
    () => markdownMode ? markdownImagePreview(markdownImages) : null,
    [markdownImages, markdownMode],
  );

  const baseExtensions = useMemo<Extension[]>(() => [
    workspaceEditorTheme,
    syntaxHighlighting(workspaceHighlightStyle),
    EditorView.lineWrapping,
    EditorState.readOnly.of(!editable),
    EditorView.editable.of(editable),
    Prec.highest(keymap.of([{ key: 'Mod-s', preventDefault: true, run: () => { onSaveRef.current(); return true; } }])),
    ...(languageExtension ? [languageExtension] : []),
  ], [editable, languageExtension]);
  const previewExtensions = useMemo<Extension[]>(() => [
    ...baseExtensions,
    ...markdownExtensions,
    ...(markdownImageExtension ? [markdownImageExtension] : []),
  ], [baseExtensions, markdownExtensions, markdownImageExtension]);
  const activeEditorRef = markdownMode === 'source' ? sourceEditorRef : editorRef;

  useEffect(() => {
    if (!previewMode) return;
    const frame = requestAnimationFrame(() => editorRef.current?.view?.requestMeasure());
    return () => cancelAnimationFrame(frame);
  }, [previewMode]);

  useEffect(() => {
    const view = activeEditorRef.current?.view;
    if (!view || !target?.line) {
      onLocationAdjusted?.(false);
      return;
    }
    const lineNumber = Math.min(Math.max(1, target.line), view.state.doc.lines);
    const line = view.state.doc.line(lineNumber);
    const columnOffset = Math.min(Math.max(0, (target.column ?? 1) - 1), line.length);
    const adjusted = target.line !== lineNumber
      || (target.column != null && target.column - 1 !== columnOffset)
      || (target.endLine != null && target.endLine > view.state.doc.lines);
    onLocationAdjusted?.(adjusted);
    const anchor = line.from + columnOffset;
    let head = anchor;
    if (target.endLine && target.endLine >= lineNumber) {
      head = view.state.doc.line(Math.min(target.endLine, view.state.doc.lines)).to;
    }
    view.dispatch({
      selection: EditorSelection.single(anchor, head),
      effects: EditorView.scrollIntoView(anchor, { y: 'center' }),
    });
    view.focus();
  }, [contentRevision, markdownMode, onLocationAdjusted, target, targetRevision]);

  useEffect(() => () => {
    if (copiedTimerRef.current) clearTimeout(copiedTimerRef.current);
    const state = activeEditorRef.current?.view?.state;
    if (state) onPersistState(state.toJSON({ history: historyField }));
  }, [markdownMode, onPersistState]);

  const copySource = async () => {
    const source = activeEditorRef.current?.view?.state.doc.toString() ?? value;
    await navigator.clipboard.writeText(source);
    setCopied(true);
    if (copiedTimerRef.current) clearTimeout(copiedTimerRef.current);
    copiedTimerRef.current = setTimeout(() => setCopied(false), 1_500);
  };

  const switchMarkdownMode = () => {
    const state = activeEditorRef.current?.view?.state;
    if (state) onPersistState(state.toJSON({ history: historyField }));
    onMarkdownModeChange?.(previewMode ? 'source' : 'live-preview');
  };

  return (
    <div className="relative h-full min-h-0">
      {markdownMode ? (
        <div className="absolute right-3 top-2 z-20 flex items-center gap-1 rounded-md border border-border/50 bg-background/88 p-1 shadow-sm backdrop-blur">
          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                size="icon"
                variant="ghost"
                className="size-7"
                onClick={() => void copySource()}
                aria-label={t('workspace.filesPanel.copyMarkdownSource')}
              >
                {copied ? <Check className="size-3.5 text-emerald-600" /> : <Copy className="size-3.5" />}
              </Button>
            </TooltipTrigger>
            <TooltipContent>{t(copied ? 'workspace.filesPanel.markdownSourceCopied' : 'workspace.filesPanel.copyMarkdownSource')}</TooltipContent>
          </Tooltip>
          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                size="icon"
                variant="ghost"
                className="size-7"
                disabled={!markdownLivePreviewAvailable && markdownMode === 'source'}
                onClick={switchMarkdownMode}
                aria-label={t(previewMode ? 'workspace.filesPanel.viewMarkdownSource' : 'workspace.filesPanel.viewMarkdownLivePreview')}
              >
                {previewMode ? <Code2 className="size-3.5" /> : <Eye className="size-3.5" />}
              </Button>
            </TooltipTrigger>
            <TooltipContent>{t(previewMode ? 'workspace.filesPanel.viewMarkdownSource' : 'workspace.filesPanel.viewMarkdownLivePreview')}</TooltipContent>
          </Tooltip>
        </div>
      ) : null}
      {markdownMode ? (
        <>
          {markdownExtensionsReady ? (
            <div className={previewMode ? 'h-full min-h-0' : 'hidden'} aria-hidden={!previewMode}>
              <CodeMirror
                ref={editorRef}
                value={value}
                height="100%"
                theme="none"
                basicSetup={{
                  lineNumbers: false,
                  foldGutter: false,
                  highlightActiveLine: false,
                  highlightSelectionMatches: highlight,
                  searchKeymap: true,
                }}
                extensions={previewExtensions}
                initialState={initialStateJson ? { json: initialStateJson, fields: { history: historyField } } : undefined}
                onChange={(nextValue) => {
                  if (nextValue !== value) onChange(nextValue);
                }}
                onBlur={() => onSaveRef.current()}
                className="atomic-cm-editor workspace-markdown-live-preview h-full min-h-0 overflow-hidden [&_.cm-editor]:h-full [&_.cm-scroller]:overflow-auto"
                aria-label="workspace-file-editor"
              />
            </div>
          ) : previewMode ? (
            <div className="flex h-full items-center justify-center text-sm text-muted-foreground" aria-label="workspace-markdown-loading">…</div>
          ) : null}
          {!previewMode ? (
            <CodeMirror
              ref={sourceEditorRef}
              value={value}
              height="100%"
              theme="none"
              basicSetup={{
                lineNumbers: true,
                foldGutter: highlight,
                highlightActiveLine: true,
                highlightSelectionMatches: highlight,
                searchKeymap: true,
              }}
              extensions={baseExtensions}
              initialState={initialStateJson ? { json: initialStateJson, fields: { history: historyField } } : undefined}
              onChange={(nextValue) => {
                if (nextValue !== value) onChange(nextValue);
              }}
              onBlur={() => onSaveRef.current()}
              className="h-full min-h-0 overflow-hidden [&_.cm-editor]:h-full [&_.cm-scroller]:overflow-auto"
              aria-label="workspace-file-editor"
            />
          ) : null}
        </>
      ) : <CodeMirror
        ref={editorRef}
        value={value}
        height="100%"
        theme="none"
        basicSetup={{
          lineNumbers: !previewMode,
          foldGutter: highlight && !previewMode,
          highlightActiveLine: !previewMode,
          highlightSelectionMatches: highlight,
          searchKeymap: true,
        }}
        extensions={baseExtensions}
        initialState={initialStateJson ? { json: initialStateJson, fields: { history: historyField } } : undefined}
        onChange={(nextValue) => {
          if (nextValue !== value) onChange(nextValue);
        }}
        onBlur={() => onSaveRef.current()}
        className="h-full min-h-0 overflow-hidden [&_.cm-editor]:h-full [&_.cm-scroller]:overflow-auto"
        aria-label="workspace-file-editor"
      />}
    </div>
  );
}
