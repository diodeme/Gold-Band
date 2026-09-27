import { describe, expect, it } from 'vitest';
import {
  MAX_COMPOSER_DIFF_QUOTE_BYTES,
  MAX_COMPOSER_QUOTE_CHARS,
  MAX_COMPOSER_QUOTES,
  MAX_COMPOSER_CONTEXT_ITEMS,
  addComposerQuote,
  addComposerWorkspaceFile,
  createUserPromptSubmission,
  hasUserPromptPayload,
  userPromptQuotesFromRaw,
  userPromptRoleFromRaw,
  workspaceFilesFromRaw,
  type ComposerQuote,
  type ComposerWorkspaceFileRef,
} from '@/lib/composer-context';

const quote = (id: string, text: string, messageKey = id): ComposerQuote => ({
  id,
  text,
  source: { kind: 'agentMessage', messageKey },
});
const wholeDiff = (id: string, text: string, path = 'src/a.ts'): ComposerQuote => ({
  id,
  text,
  source: { kind: 'diff', path, origin: 'agentTurn', revision: null, scope: 'file' },
});
const workspaceFile = (path: string, id = 'ref-1'): ComposerWorkspaceFileRef => ({
  id,
  projectId: 'project-1',
  relativePath: path,
  name: path.split('/').at(-1) ?? path,
  byteLength: 12,
  mimeType: 'text/typescript',
  canonicalPath: 'D:/workspace/' + path,
});

describe('composer quote contract', () => {
  it('keeps display text and structured quotes separate from the agent prompt', () => {
    const first = addComposerQuote([], quote('one', '\n第一行\n第二行  '));
    expect(first.ok).toBe(true);
    if (!first.ok) return;
    const second = addComposerQuote(first.quotes, {
      id: 'two',
      text: '  indented()',
      source: { kind: 'file', label: 'src/a.ts', startLine: 3, endLine: 3 },
    });
    expect(second.ok).toBe(true);
    if (!second.ok) return;
    expect(createUserPromptSubmission('继续解释', second.quotes)).toEqual({
      displayText: '继续解释',
      quotes: [
        { id: 'one', text: '第一行\n第二行', source: { kind: 'agentMessage', messageKey: 'one' } },
        { id: 'two', text: '  indented()', source: { kind: 'file', label: 'src/a.ts', startLine: 3, endLine: 3 } },
      ],
      workspaceFiles: [],
    });
  });

  it('keeps a role snapshot on the submission without putting it in display text', () => {
    const submission = createUserPromptSubmission('帮我改代码', [], {
      profileId: 'pf-dev',
      name: '开发',
      content: '完整角色定义',
    });
    expect(submission.displayText).toBe('帮我改代码');
    expect(submission.role).toEqual({
      profileId: 'pf-dev',
      name: '开发',
      content: '完整角色定义',
    });
    expect(userPromptRoleFromRaw({ role: submission.role })).toEqual(submission.role);
  });

  it('does not infer quotes from user-authored Markdown blockquotes', () => {
    const submission = createUserPromptSubmission('> 这是用户自己输入的引用格式', []);
    expect(submission.displayText).toBe('> 这是用户自己输入的引用格式');
    expect(submission.quotes).toEqual([]);
    expect(userPromptQuotesFromRaw({ source: 'goldBandPrompt' })).toEqual([]);
  });

  it('only reads valid explicit quote metadata', () => {
    expect(userPromptQuotesFromRaw({
      quotes: [
        { id: 'one', text: '引用内容', source: { kind: 'agentMessage', messageKey: 'message-1' } },
        { id: 'two', text: '', source: { kind: 'diff', path: 'a.ts', origin: 'commit', revision: 'abc', scope: 'file' } },
        { id: 'three', text: '', source: { kind: 'diff', path: 'a.ts', origin: 'commit', revision: 'abc', scope: 'selection' } },
        { id: 'four', text: 'x', source: { kind: 'file', label: 'a.ts', startLine: 0, endLine: 1 } },
        { id: '', text: 'invalid', source: { kind: 'agentMessage', messageKey: 'message-2' } },
        { id: 'legacy', sourceMessageKey: 'message-3', text: 'legacy' },
        { id: 'legacy-without-key', text: 'legacy' },
      ],
    })).toEqual([
      { id: 'one', text: '引用内容', source: { kind: 'agentMessage', messageKey: 'message-1' } },
      { id: 'two', text: '', source: { kind: 'diff', path: 'a.ts', origin: 'commit', revision: 'abc', scope: 'file' } },
      { id: 'legacy', text: 'legacy', source: { kind: 'agentMessage', messageKey: 'message-3' } },
    ]);
  });

  it('rejects the same selection from the same source', () => {
    const existing = [quote('one', '相同内容', 'message-1')];
    expect(addComposerQuote(existing, quote('two', '相同内容', 'message-1'))).toMatchObject({
      ok: false,
      code: 'composer.quote.duplicate',
    });
    expect(addComposerQuote(existing, quote('two', '相同内容', 'message-2')).ok).toBe(true);
  });

  it('keeps the draft unchanged and reports the remaining characters when a quote is over budget', () => {
    const existing = [quote('one', 'a'.repeat(MAX_COMPOSER_QUOTE_CHARS - 5))];
    expect(addComposerQuote(existing, quote('two', 'b'.repeat(8)))).toEqual({
      ok: false,
      code: 'composer.quote.limit-exceeded',
      chars: 8,
      maxChars: MAX_COMPOSER_QUOTE_CHARS,
      remainingChars: 5,
    });
    expect(existing).toHaveLength(1);
  });

  it('charges whole-file diffs to their own per-message byte budget', () => {
    const big = 'x'.repeat(MAX_COMPOSER_DIFF_QUOTE_BYTES - 10);
    const first = addComposerQuote([quote('one', 'a'.repeat(MAX_COMPOSER_QUOTE_CHARS))], wholeDiff('diff-1', big));
    expect(first.ok).toBe(true);
    if (!first.ok) return;
    expect(addComposerQuote(first.quotes, wholeDiff('diff-2', '文'.repeat(4), 'src/b.ts'))).toEqual({
      ok: false,
      code: 'composer.quote.diff-limit-exceeded',
      bytes: 12,
      maxBytes: MAX_COMPOSER_DIFF_QUOTE_BYTES,
      remainingBytes: 10,
    });
  });

  it('keeps the quote list bounded independently of the character budget', () => {
    const existing = Array.from({ length: MAX_COMPOSER_QUOTES }, (_, index) =>
      quote(`quote-${index}`, 'x', `message-${index}`),
    );

    expect(addComposerQuote(existing, quote('overflow', 'x', 'overflow'))).toEqual({
      ok: false,
      code: 'composer.quote.count-exceeded',
      maxQuotes: MAX_COMPOSER_QUOTES,
    });
  });
});

describe('composer payload contract', () => {
  it('allows text-only and attachment-only payloads while rejecting a completely empty draft', () => {
    expect(hasUserPromptPayload('hello', 0)).toBe(true);
    expect(hasUserPromptPayload('', 1)).toBe(true);
    expect(hasUserPromptPayload('   ', 0)).toBe(false);
  });

  it('treats a complete role snapshot as a sendable payload even without user text', () => {
    const role = { profileId: 'pf-dev', name: '开发', content: '完整角色定义' };
    expect(hasUserPromptPayload('', 0, role)).toBe(true);
    expect(hasUserPromptPayload('   ', 0, role)).toBe(true);
    expect(hasUserPromptPayload('', 0, { profileId: 'pf-dev', name: '开发', content: '' })).toBe(false);
    const submission = createUserPromptSubmission('', [], role);
    expect(submission.displayText).toBe('');
    expect(submission.role).toEqual(role);
  });

  it('keeps workspace file references as a separate sendable context', () => {
    const first = addComposerWorkspaceFile([], 0, workspaceFile('src\\WorkspaceFileTree.tsx'));
    expect(first).toMatchObject({ ok: true });
    if (!first.ok) return;
    const duplicate = addComposerWorkspaceFile(
      first.workspaceFiles,
      0,
      workspaceFile('src/WorkspaceFileTree.tsx', 'ref-2'),
    );
    expect(duplicate).toMatchObject({ ok: false, code: 'composer.workspace-file.duplicate' });

    const submission = createUserPromptSubmission('', [], null, first.workspaceFiles);
    expect(submission.displayText).toBe('');
    expect(submission.workspaceFiles).toEqual([
      { projectId: 'project-1', relativePath: 'src/WorkspaceFileTree.tsx' },
    ]);
    expect(hasUserPromptPayload('', 0, null, first.workspaceFiles.length)).toBe(true);
  });

  it('bounds workspace references and attachments by one composer context limit', () => {
    const files = Array.from({ length: MAX_COMPOSER_CONTEXT_ITEMS }, (_, index) =>
      workspaceFile(`src/file-${index}.ts`, `ref-${index}`));
    expect(addComposerWorkspaceFile(files.slice(0, 9), 1, workspaceFile('src/overflow.ts'))).toEqual({
      ok: false,
      code: 'composer.context.limit-exceeded',
      max: MAX_COMPOSER_CONTEXT_ITEMS,
    });
  });

  it('defensively parses canonical workspace file metadata', () => {
    expect(workspaceFilesFromRaw({
      workspaceFiles: [
        {
          projectId: 'project-1',
          relativePath: 'src/a.ts',
          canonicalPath: 'D:/workspace/src/a.ts',
          name: 'a.ts',
          mimeType: 'text/typescript',
          size: 12,
        },
        { projectId: '', relativePath: 'src/b.ts' },
        { projectId: 'project-1' },
      ],
    })).toEqual([{
      projectId: 'project-1',
      relativePath: 'src/a.ts',
      canonicalPath: 'D:/workspace/src/a.ts',
      name: 'a.ts',
      mimeType: 'text/typescript',
      size: 12,
    }]);
    expect(workspaceFilesFromRaw({ workspaceFiles: 'invalid' })).toEqual([]);
  });
});
