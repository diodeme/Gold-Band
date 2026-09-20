import { describe, expect, it } from 'vitest';
import {
  MAX_COMPOSER_QUOTE_CHARS,
  MAX_COMPOSER_QUOTES,
  MAX_COMPOSER_CONTEXT_ITEMS,
  addComposerQuote,
  addComposerWorkspaceFile,
  createUserPromptSubmission,
  hasUserPromptPayload,
  serializeUserPromptSubmission,
  userPromptQuotesFromRaw,
  userPromptRoleFromRaw,
  workspaceFilesFromRaw,
  type ComposerQuote,
  type ComposerWorkspaceFileRef,
} from '@/lib/composer-context';

const quote = (id: string, text: string, sourceKey = id): ComposerQuote => ({ id, text, sourceKey });
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
    const first = addComposerQuote([], quote('one', '第一行\n第二行'));
    expect(first.ok).toBe(true);
    if (!first.ok) return;
    const second = addComposerQuote(first.quotes, quote('two', '另一段'));
    expect(second.ok).toBe(true);
    if (!second.ok) return;
    const submission = createUserPromptSubmission('继续解释', second.quotes);
    expect(submission).toEqual({
      displayText: '继续解释',
      quotes: [
        { id: 'one', sourceMessageKey: 'one', text: '第一行\n第二行' },
        { id: 'two', sourceMessageKey: 'two', text: '另一段' },
      ],
      workspaceFiles: [],
    });
    expect(serializeUserPromptSubmission(submission)).toBe('> 第一行\n> 第二行\n\n> 另一段\n\n继续解释');
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
        { id: 'one', sourceMessageKey: 'message-1', text: '引用内容' },
        { id: '', sourceMessageKey: 'message-2', text: 'invalid' },
      ],
    })).toEqual([{ id: 'one', sourceMessageKey: 'message-1', text: '引用内容' }]);
  });

  it('rejects the same selection from the same source message', () => {
    const existing = [quote('one', '相同内容', 'message-1')];
    expect(addComposerQuote(existing, quote('two', '相同内容', 'message-1'))).toMatchObject({
      ok: false,
      code: 'composer.quote.duplicate',
    });
  });

  it('keeps the existing draft unchanged when the total character limit would be exceeded', () => {
    const existing = [quote('one', 'a'.repeat(MAX_COMPOSER_QUOTE_CHARS))];
    expect(addComposerQuote(existing, quote('two', 'b'))).toEqual({
      ok: false,
      code: 'composer.quote.limit-exceeded',
      maxChars: MAX_COMPOSER_QUOTE_CHARS,
    });
    expect(existing).toHaveLength(1);
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
