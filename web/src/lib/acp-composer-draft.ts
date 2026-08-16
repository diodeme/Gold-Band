import { useCallback, useReducer, useRef, type Dispatch, type SetStateAction } from 'react';
import { revokeAttachmentPreviewUrls, type AttachmentItem } from './attachment-service';
import type { ComposerQuote } from './composer-context';

export interface AcpComposerDraft {
  content: string;
  attachments: AttachmentItem[];
  quotes: ComposerQuote[];
}

export const MAX_ACP_COMPOSER_DRAFTS = 64;
export const MAX_ACP_COMPOSER_DRAFT_ATTACHMENT_BYTES = 100 * 1024 * 1024;

function emptyDraft(): AcpComposerDraft {
  return { content: '', attachments: [], quotes: [] };
}

function hasDraftContent(draft: AcpComposerDraft) {
  return draft.content.length > 0 || draft.attachments.length > 0 || draft.quotes.length > 0;
}

function attachmentBytes(draft: AcpComposerDraft) {
  return draft.attachments.reduce((total, attachment) => total + attachment.size, 0);
}

/** Process-lifetime canonical store for unsubmitted ACP follow-up drafts. */
export class AcpComposerDraftStore {
  private readonly entries = new Map<string, AcpComposerDraft>();

  constructor(
    private readonly maxDrafts = MAX_ACP_COMPOSER_DRAFTS,
    private readonly maxAttachmentBytes = MAX_ACP_COMPOSER_DRAFT_ATTACHMENT_BYTES,
  ) {}

  get size() {
    return this.entries.size;
  }

  read(key: string): AcpComposerDraft {
    const draft = this.entries.get(key);
    if (!draft) return emptyDraft();
    this.entries.delete(key);
    this.entries.set(key, draft);
    return draft;
  }

  write(key: string, draft: AcpComposerDraft) {
    this.entries.delete(key);
    if (hasDraftContent(draft)) this.entries.set(key, draft);
    this.evictOverflow(key);
  }

  restoreIfEmpty(key: string, draft: AcpComposerDraft) {
    if (hasDraftContent(this.entries.get(key) ?? emptyDraft())) return false;
    this.write(key, draft);
    return true;
  }

  dispose() {
    for (const draft of this.entries.values()) revokeAttachmentPreviewUrls(draft.attachments);
    this.entries.clear();
  }

  private evictOverflow(activeKey: string) {
    let totalBytes = [...this.entries.values()].reduce(
      (total, draft) => total + attachmentBytes(draft),
      0,
    );
    while (this.entries.size > this.maxDrafts || totalBytes > this.maxAttachmentBytes) {
      const oldestKey = this.entries.keys().next().value as string | undefined;
      if (oldestKey === undefined) break;
      if (oldestKey === activeKey && this.entries.size === 1) break;
      const oldest = this.entries.get(oldestKey);
      this.entries.delete(oldestKey);
      if (oldest) {
        totalBytes -= attachmentBytes(oldest);
        revokeAttachmentPreviewUrls(oldest.attachments);
      }
    }
  }
}

const acpComposerDraftStore = new AcpComposerDraftStore();

export function disposeAcpComposerDrafts() {
  acpComposerDraftStore.dispose();
}

export interface AcpComposerDraftController {
  draft: AcpComposerDraft;
  setContent: Dispatch<SetStateAction<string>>;
  setAttachments: Dispatch<SetStateAction<AttachmentItem[]>>;
  setQuotes: Dispatch<SetStateAction<ComposerQuote[]>>;
  clearIfUnchanged: (expected: AcpComposerDraft) => boolean;
  restoreIfEmpty: (draft: AcpComposerDraft) => boolean;
}

export function useAcpComposerDraft(key: string): AcpComposerDraftController {
  const keyRef = useRef(key);
  const draftRef = useRef<AcpComposerDraft | null>(null);
  if (!draftRef.current) draftRef.current = acpComposerDraftStore.read(key);
  const [, renderCurrentDraft] = useReducer((revision: number) => revision + 1, 0);
  if (keyRef.current !== key) {
    keyRef.current = key;
    draftRef.current = acpComposerDraftStore.read(key);
  }

  const setContent = useCallback<Dispatch<SetStateAction<string>>>((next) => {
    const current = draftRef.current ?? emptyDraft();
    const content = typeof next === 'function' ? next(current.content) : next;
    if (content === current.content) return;
    const updated = { ...current, content };
    draftRef.current = updated;
    acpComposerDraftStore.write(key, updated);
    renderCurrentDraft();
  }, [key]);

  const setAttachments = useCallback<Dispatch<SetStateAction<AttachmentItem[]>>>((next) => {
    const current = draftRef.current ?? emptyDraft();
    const attachments = typeof next === 'function' ? next(current.attachments) : next;
    if (attachments === current.attachments) return;
    const updated = { ...current, attachments };
    draftRef.current = updated;
    acpComposerDraftStore.write(key, updated);
    renderCurrentDraft();
  }, [key]);

  const setQuotes = useCallback<Dispatch<SetStateAction<ComposerQuote[]>>>((next) => {
    const current = draftRef.current ?? emptyDraft();
    const quotes = typeof next === 'function' ? next(current.quotes) : next;
    if (quotes === current.quotes) return;
    const updated = { ...current, quotes };
    draftRef.current = updated;
    acpComposerDraftStore.write(key, updated);
    renderCurrentDraft();
  }, [key]);

  const clearIfUnchanged = useCallback((expected: AcpComposerDraft) => {
    if (draftRef.current !== expected) return false;
    const cleared = emptyDraft();
    draftRef.current = cleared;
    acpComposerDraftStore.write(key, cleared);
    renderCurrentDraft();
    return true;
  }, [key]);

  const restoreIfEmpty = useCallback((draft: AcpComposerDraft) => {
    if (!acpComposerDraftStore.restoreIfEmpty(key, draft)) return false;
    if (keyRef.current === key) {
      draftRef.current = draft;
      renderCurrentDraft();
    }
    return true;
  }, [key]);

  return {
    draft: draftRef.current ?? emptyDraft(),
    setContent,
    setAttachments,
    setQuotes,
    clearIfUnchanged,
    restoreIfEmpty,
  };
}
