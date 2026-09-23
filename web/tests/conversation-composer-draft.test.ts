import { afterEach, describe, expect, it, vi } from 'vitest';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import {
  conversationComposerDraftReducer,
  createConversationComposerDraftBoundaryHandle,
  createInitialConversationComposerDraft,
  resetConversationComposerDraft,
  type ConversationComposerDraftState,
  type ConversationComposerRemoteBinding,
} from '../src/lib/conversation-composer-draft';
import { revokeAttachmentPreviewUrls, type AttachmentItem } from '../src/lib/attachment-service';

/**
 * 回归测试：会话发起 composer 的未提交草稿（正文、附件与提交模式）在离开
 * 会话主页再返回后必须保留。
 *
 * 真实场景里，跳转运行模式管理、设置页或其他会话窗口都会卸载
 * ConversationComposer，但其草稿已上提为 App 层 owner 状态，存活期独立于
 * 组件挂载。这里覆盖驱动该状态的纯函数 reducer 语义，以及图片预览 URL
 * 只在 owner 清理路径释放的资源语义。
 */
function makeAttachment(id: string): AttachmentItem {
  return { id, name: `${id}.txt`, size: 1, mime: 'text/plain', source: 'dialog' };
}

function makeImageAttachment(id: string): AttachmentItem {
  return {
    id,
    name: `${id}.png`,
    size: 1,
    mime: 'image/png',
    source: 'browser-file',
    previewUrl: `blob:${id}`,
  };
}

function makeWorkspaceFile(id: string, projectId: string, relativePath: string) {
  return {
    id,
    projectId,
    relativePath,
    name: relativePath.split('/').at(-1) ?? relativePath,
    byteLength: 1,
    mimeType: 'text/plain',
  };
}

describe('ConversationComposer draft cross-page retention', () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('initial draft is empty', () => {
    expect(createInitialConversationComposerDraft()).toEqual({ content: '', attachments: [], workspaceFiles: [], remote: null, submission: { kind: 'send' } });
  });

  it('setContent stores text without losing attachments', () => {
    const state: ConversationComposerDraftState = { content: '', attachments: [makeAttachment('a1')], workspaceFiles: [], remote: null, submission: { kind: 'send' } };
    const next = conversationComposerDraftReducer(state, { type: 'setContent', content: 'hello' });
    expect(next.content).toBe('hello');
    expect(next.attachments).toHaveLength(1);
  });

  it('setAttachments stores attachments without losing text', () => {
    const state: ConversationComposerDraftState = { content: 'hello', attachments: [], workspaceFiles: [], remote: null, submission: { kind: 'send' } };
    const next = conversationComposerDraftReducer(state, { type: 'setAttachments', attachments: [makeAttachment('a1'), makeAttachment('a2')] });
    expect(next.content).toBe('hello');
    expect(next.attachments.map((a) => a.id)).toEqual(['a1', 'a2']);
  });

  it('changing workspace keeps references from other workspaces', () => {
    const state: ConversationComposerDraftState = {
      content: '继续处理',
      attachments: [makeAttachment('a1')],
      workspaceFiles: [
        makeWorkspaceFile('project-a-file', 'project-a', 'src/a.ts'),
        makeWorkspaceFile('project-b-file', 'project-b', 'src/b.ts'),
      ],
      remote: null,
      submission: { kind: 'send' },
    };

    const next = conversationComposerDraftReducer(state, {
      type: 'changeWorkspace',
      projectId: 'project-b',
    });

    expect(next).toBe(state);
    expect(next.workspaceFiles.map(file => file.id)).toEqual(['project-a-file', 'project-b-file']);
  });

  it('changing to the same workspace is a no-op (stable reference)', () => {
    const state: ConversationComposerDraftState = {
      content: 'keep',
      attachments: [],
      workspaceFiles: [makeWorkspaceFile('project-a-file', 'project-a', 'src/a.ts')],
      remote: null,
      submission: { kind: 'send' },
    };

    const next = conversationComposerDraftReducer(state, {
      type: 'changeWorkspace',
      projectId: 'project-a',
    });

    expect(next).toBe(state);
  });

  it('setContent with identical value is a no-op (stable reference)', () => {
    const state: ConversationComposerDraftState = { content: 'same', attachments: [], workspaceFiles: [], remote: null, submission: { kind: 'send' } };
    const next = conversationComposerDraftReducer(state, { type: 'setContent', content: 'same' });
    expect(next).toBe(state);
  });

  /**
   * 模拟离开会话主页再返回：owner 状态本身不随 composer 卸载而改变，
   * 因此 reducer 在两次 setContent 之间不需要任何中间清理即可保留正文。
   */
  it('retains content across a simulated unmount/remount (owner state persists)', () => {
    let state = createInitialConversationComposerDraft();
    // 用户输入正文，尚未发送
    state = conversationComposerDraftReducer(state, { type: 'setContent', content: '未发送的草稿' });
    // 模拟组件卸载（跳转配置/设置/其他会话）后再挂载：owner 状态不受影响，content 仍在
    expect(state.content).toBe('未发送的草稿');
    // 返回后继续编辑
    state = conversationComposerDraftReducer(state, { type: 'setContent', content: '未发送的草稿，继续' });
    expect(state.content).toBe('未发送的草稿，继续');
  });

  it('retains attachments across a simulated unmount/remount', () => {
    let state = createInitialConversationComposerDraft();
    state = conversationComposerDraftReducer(state, { type: 'setAttachments', attachments: [makeAttachment('img')] });
    // 模拟跳转再返回：附件仍在
    expect(state.attachments).toHaveLength(1);
    expect(state.attachments[0].id).toBe('img');
  });

  it('retains scheduled-task mode and its configured schedule across a simulated unmount/remount', () => {
    let state = createInitialConversationComposerDraft();
    state = conversationComposerDraftReducer(state, { type: 'enterScheduledTask' });
    state = conversationComposerDraftReducer(state, {
      type: 'setScheduledTaskConfig',
      config: {
        schedule: { kind: 'Repeat', preset: 'Daily', hour: 9, minute: 0, timezone: 'Asia/Hong_Kong' },
        overlapPolicy: 'skip_when_running',
        sessionPolicy: 'new',
      },
    });

    expect(state.submission).toEqual({
      kind: 'scheduled-task',
      config: {
        schedule: { kind: 'Repeat', preset: 'Daily', hour: 9, minute: 0, timezone: 'Asia/Hong_Kong' },
        overlapPolicy: 'skip_when_running',
        sessionPolicy: 'new',
      },
    });

    state = conversationComposerDraftReducer(state, { type: 'setContent', content: '返回后继续编辑' });
    expect(state.submission.kind).toBe('scheduled-task');
  });

  it('exits scheduled-task mode without clearing the ordinary composer payload', () => {
    let state = createInitialConversationComposerDraft();
    state = conversationComposerDraftReducer(state, { type: 'setContent', content: '保留正文' });
    state = conversationComposerDraftReducer(state, { type: 'enterScheduledTask' });
    state = conversationComposerDraftReducer(state, { type: 'exitScheduledTask' });

    expect(state).toEqual({ content: '保留正文', attachments: [], workspaceFiles: [], remote: null, submission: { kind: 'send' } });
  });

  it('does not revoke image preview URLs during ordinary cross-page retention', () => {
    const revokeSpy = vi.spyOn(URL, 'revokeObjectURL').mockImplementation(() => {});
    const attachment = makeImageAttachment('img');
    let state: ConversationComposerDraftState = { content: 'x', attachments: [attachment], workspaceFiles: [], remote: null, submission: { kind: 'send' } };

    state = conversationComposerDraftReducer(state, { type: 'setContent', content: 'x after navigation' });

    expect(state.attachments[0].previewUrl).toBe('blob:img');
    expect(revokeSpy).not.toHaveBeenCalled();
  });

  it('releases preview URLs through the owner cleanup helper', () => {
    const revokeSpy = vi.spyOn(URL, 'revokeObjectURL').mockImplementation(() => {});

    revokeAttachmentPreviewUrls([makeAttachment('plain'), makeImageAttachment('img')]);

    expect(revokeSpy).toHaveBeenCalledTimes(1);
    expect(revokeSpy).toHaveBeenCalledWith('blob:img');
  });

  it('reset clears content and attachments (used after successful create)', () => {
    let state: ConversationComposerDraftState = { content: 'x', attachments: [makeAttachment('a1')], workspaceFiles: [], remote: null, submission: { kind: 'send' } };
    state = conversationComposerDraftReducer(state, { type: 'reset' });
    expect(state).toEqual({ content: '', attachments: [], workspaceFiles: [], remote: null, submission: { kind: 'send' } });
  });

  it('reset clears a scheduled-task submission mode as well', () => {
    let state: ConversationComposerDraftState = { content: 'x', attachments: [makeAttachment('a1')], workspaceFiles: [], remote: null, submission: { kind: 'scheduled-task', config: null } };
    state = conversationComposerDraftReducer(state, { type: 'reset' });
    expect(state).toEqual({ content: '', attachments: [], workspaceFiles: [], remote: null, submission: { kind: 'send' } });
  });

  it('does not reset the shared draft from either workspace selector path', () => {
    const appSource = readFileSync(fileURLToPath(new URL('../src/App.tsx', import.meta.url)), 'utf8');
    expect(appSource.match(/onWorkspaceChange=\{\(projectId\) => \{/g)).toHaveLength(2);
    expect(appSource.match(/onWorkspaceChange=\{\(projectId\) => \{[^}]*resetConversationComposerDraft/g)).toBeNull();
  });

  it('prefill writes requirement text + remote binding and drops prior attachments and workspace files', () => {
    const state: ConversationComposerDraftState = {
      content: '本地草稿',
      attachments: [makeAttachment('a1')],
      workspaceFiles: [makeWorkspaceFile('project-a-file', 'project-a', 'src/a.ts')],
      remote: null,
      submission: { kind: 'send' },
    };
    const binding: ConversationComposerRemoteBinding = { source: 'multica', remoteTaskId: 'rt-1', workspaceId: 'ws-1', title: 'T1' };
    const next = conversationComposerDraftReducer(state, { type: 'prefill', content: '远程任务需求正文', remote: binding });

    expect(next.content).toBe('远程任务需求正文');
    // 远程任务草稿不复用上一条本地草稿的附件与工作区文件引用（远程任务在远程工作空间执行）。
    expect(next.attachments).toEqual([]);
    expect(next.workspaceFiles).toEqual([]);
    expect(next.remote).toEqual(binding);
  });

  it('editing prefilled content keeps the remote binding (setContent preserves the binding)', () => {
    const binding: ConversationComposerRemoteBinding = { source: 'multica', remoteTaskId: 'rt-1', workspaceId: 'ws-1', title: 'T1' };
    let state = conversationComposerDraftReducer(
      { content: '需求', attachments: [], workspaceFiles: [], remote: null, submission: { kind: 'send' } },
      { type: 'prefill', content: '需求', remote: binding },
    );
    // 用户在预填基础上微调正文。
    state = conversationComposerDraftReducer(state, { type: 'setContent', content: '需求（改）' });

    expect(state.content).toBe('需求（改）');
    // 绑定仍在 → 发送时仍走远程分支。
    expect(state.remote).toEqual(binding);
  });

  it('clearRemote drops the binding but keeps content and attachments (chip removed → local compose)', () => {
    const binding: ConversationComposerRemoteBinding = { source: 'multica', remoteTaskId: 'rt-1', workspaceId: 'ws-1', title: 'T1' };
    const state: ConversationComposerDraftState = { content: '需求正文', attachments: [makeAttachment('a1')], workspaceFiles: [], remote: binding, submission: { kind: 'send' } };
    const next = conversationComposerDraftReducer(state, { type: 'clearRemote' });

    // 解绑后正文与附件保留，仅 remote 置空 → 发送降级为普通本地会话。
    expect(next.remote).toBeNull();
    expect(next.content).toBe('需求正文');
    expect(next.attachments.map((a) => a.id)).toEqual(['a1']);
  });

  it('clearRemote is a no-op when no binding is present (stable reference)', () => {
    const state: ConversationComposerDraftState = { content: 'x', attachments: [], workspaceFiles: [], remote: null, submission: { kind: 'send' } };
    const next = conversationComposerDraftReducer(state, { type: 'clearRemote' });
    expect(next).toBe(state);
  });

  it('reset clears a leftover remote binding so the next local compose is not mistaken for a remote run', () => {
    const binding: ConversationComposerRemoteBinding = { source: 'multica', remoteTaskId: 'rt-1', workspaceId: 'ws-1', title: 'T1' };
    let state: ConversationComposerDraftState = { content: '需求', attachments: [], workspaceFiles: [], remote: binding, submission: { kind: 'send' } };
    state = conversationComposerDraftReducer(state, { type: 'reset' });

    expect(state.remote).toBeNull();
    expect(state.content).toBe('');
  });

  it('prefill from scheduled-task mode returns the draft to send intent (remote prepare owns the draft)', () => {
    const binding: ConversationComposerRemoteBinding = { source: 'multica', remoteTaskId: 'rt-1', workspaceId: 'ws-1', title: 'T1' };
    let state = createInitialConversationComposerDraft();
    state = conversationComposerDraftReducer(state, { type: 'enterScheduledTask' });
    state = conversationComposerDraftReducer(state, { type: 'prefill', content: '远程任务需求正文', remote: binding });

    // prefill 是覆盖式新草稿：绑定生效且提交意图回到 send。
    expect(state.remote).toEqual(binding);
    expect(state.submission).toEqual({ kind: 'send' });
  });

  it('enterScheduledTask drops a remote binding (submission intents are mutually exclusive)', () => {
    const binding: ConversationComposerRemoteBinding = { source: 'multica', remoteTaskId: 'rt-1', workspaceId: 'ws-1', title: 'T1' };
    let state = createInitialConversationComposerDraft();
    state = conversationComposerDraftReducer(state, { type: 'prefill', content: '需求', remote: binding });
    state = conversationComposerDraftReducer(state, { type: 'enterScheduledTask' });

    // 排程与远程执行是互斥的提交意图：进入排程即本地解绑（任务仍在服务端 queued）。
    expect(state.remote).toBeNull();
    expect(state.submission.kind).toBe('scheduled-task');
  });

  it('exposes reset and workspace change to the App boundary handle', () => {
    const reset = vi.fn();
    const changeWorkspace = vi.fn();
    const handle = createConversationComposerDraftBoundaryHandle({
      draft: createInitialConversationComposerDraft(),
      setContent: vi.fn(),
      setAttachments: vi.fn(),
      setWorkspaceFiles: vi.fn(),
      changeWorkspace,
      prefill: vi.fn(),
      clearRemote: vi.fn(),
      enterScheduledTask: vi.fn(),
      setScheduledTaskConfig: vi.fn(),
      exitScheduledTask: vi.fn(),
      reset,
    });

    expect(Object.keys(handle)).toEqual(['reset', 'changeWorkspace']);

    resetConversationComposerDraft(handle);
    expect(reset).toHaveBeenCalledTimes(1);
    expect(changeWorkspace).not.toHaveBeenCalled();
  });

  it('ignores missing boundary handles when App reset races with unmount', () => {
    expect(() => resetConversationComposerDraft(null)).not.toThrow();
    expect(() => resetConversationComposerDraft(undefined)).not.toThrow();
  });
});
