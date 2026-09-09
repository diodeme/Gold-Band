import type { AcpUiEventVm, ConversationRunVm, RuntimeDisplayVm } from '@/types';
import type { Language } from '../site/content';

export const PREVIEW_ROUTE = '/chat/projects/default/tasks/mock-task/runs/run-052';
const sessionStartedAt = new Date(Date.now() - 62_000).toISOString();
export const taskTitle = (language: Language) => language === 'zh' ? '为工作区补充配置与说明' : 'Workspace configuration and docs';
export function previewEvents(language: Language): AcpUiEventVm[] {
  const zh = language === 'zh';
  const base = { timestamp: sessionStartedAt, raw: {} };
  return [
    { ...base, id: 'site-user', seq: 1, kind: 'userTextDelta', content: zh ? '请为这个项目完善工作区配置，并补充一份使用说明。保留每轮文件变更，方便审阅。' : 'Update the workspace configuration and add a usage guide. Keep per-turn file changes available for review.' },
    { ...base, id: 'site-plan', seq: 2, kind: 'textDelta', status: 'completed', content: zh ? '我会先检查现有配置，再更新文件并补充说明。\n\n- 检查配置字段与默认值\n- 保留每轮变更快照\n- 补充工作区使用说明' : 'I will inspect the existing configuration, update it and add documentation.\n\n- Check fields and defaults\n- Preserve per-turn change snapshots\n- Document the workspace' },
    { ...base, id: 'site-tool', seq: 3, kind: 'toolCall', title: zh ? '更新工作区文件' : 'Update workspace files', toolCallId: 'site-tool', status: 'completed', raw: { toolCallId: 'site-tool', title: zh ? '更新工作区文件' : 'Update workspace files', status: 'completed' } },
    { ...base, id: 'site-files', seq: 4, kind: 'fileChangeSet', status: 'finalized', raw: { changeSetId: 'browser-change-set-052', summary: { fileCount: 2, addedFiles: 1, modifiedFiles: 1, deletedFiles: 0, addedLines: 8, deletedLines: 2 }, attachmentCount: 0 } },
    { ...base, id: 'site-result', seq: 5, kind: 'textDelta', status: 'completed', content: zh ? '配置与文档已更新。\n\n**本轮产出**\n\n- `src/config.json`：启用文件变更快照\n- `docs/workspace-notes.md`：补充工作区说明\n\n可以从下方文件列表打开预览，逐项审阅本轮修改。' : 'Configuration and documentation are updated.\n\n**This turn**\n\n- `src/config.json`: enable file change snapshots\n- `docs/workspace-notes.md`: document the workspace\n\nOpen the files below to inspect and review the changes.' },
  ];
}

// All preview projections derive from this one bounded scenario snapshot.
export function createPreviewRun(base: ConversationRunVm, language: Language, step = 5): ConversationRunVm {
  const run = structuredClone(base);
  const done = step >= 5;
  const status = done ? 'completed' : 'running';
  const outcome = done ? 'success' : null;
  const display: RuntimeDisplayVm = { code: done ? 'success' : 'running', tone: done ? 'success' : 'running', icon: done ? 'check' : 'dot', terminal: done, resumable: false, blockingError: false };
  Object.assign(run, { runStatus: status, runOutcome: outcome, pauseReason: null, runtimeErrorMessage: null, workflowTemplateId: null, workflowGraph: { nodes: [], edges: [] }, workflowJson: '{}', workflowError: null, resumable: false });
  for (const round of run.sessionTree.rounds) {
    Object.assign(round, { status, runtimeDisplay: display });
    for (const node of round.nodes) {
      Object.assign(node, { status, runtimeDisplay: display, label: language === 'zh' ? '实现与审阅' : 'Implement and review' });
      for (const attempt of node.attempts) {
        Object.assign(attempt, { status, outcome, runtimeDisplay: display, current: true, manualCheckPending: false });
        attempt.lifecycle = {
          runtime: { status, outcome, pauseReason: null, resumable: false, current: true, active: !done, continuable: false, phase: status, revision: step },
          control: { mode: 'non-runtime-controlled' },
          acp: { sessionAvailability: 'established', liveTurnActivity: done ? 'idle' : 'running', latestTurnStatus: done ? 'completed' : 'none', stopping: false, revision: step },
          displayStatus: done ? 'success' : 'running', runtimeDisplay: display, continueKind: null,
          composer: { mode: 'normal', submitTarget: 'acp-prompt', processingKind: 'responding', statusKey: null, canStop: !done, lockInput: false },
        };
      }
    }
  }
  const session = run.selectedSession!;
  session.title = taskTitle(language);
  session.status = status;
  session.stopReason = done ? 'end_turn' : null;
  session.systemPromptAppend = null;
  session.events = previewEvents(language).slice(0, step);
  session.eventPage = { loadedCount: step, total: step, oldestSeq: step ? 1 : null, newestSeq: step || null, hasOlder: false, hasNewer: false, oldestCursor: null, newestCursor: null };
  session.timelineProjection = null;
  session.diagnostics = { rawFrameCount: 0, eventCount: step, errorCount: 0, lastError: null, lastErrorTimestamp: null };
  session.sessionStartedAt = sessionStartedAt;
  session.sessionUpdatedAt = sessionStartedAt;
  session.sessionElapsedSeconds = 62;
  return run;
}
