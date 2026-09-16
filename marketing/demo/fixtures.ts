import type { ConversationRunVm, ConversationSidebarVm, ConversationTaskRowVm, DesktopLanguage, RuntimeDisplayVm } from '@/types';
import { createPreviewRun } from '../site/fixture';
import { mockWorkflowTemplates } from '@/mockData';
import { demoAgentRegistry, demoProfiles } from './catalog';
import { demoDevelopmentFiles } from './turn-files';
import { DEMO_RUN_ID, DEMO_WORKFLOW_ROUNDS, demoWorkflowRuns } from './scenarios';

export const DEMO_TASKS = ['mock-task', 'demo-review'] as const;
export { DEMO_RUN_ID } from './scenarios';
export const DEMO_PROJECT_ID = 'default';
export const demoRunMode = (taskId: string) => taskId === 'demo-review' ? 'workflow' as const : 'direct' as const;
export function demoTitle(taskId: string, language: DesktopLanguage) {
  return taskId === 'demo-review'
    ? language === 'en' ? 'Review project structure' : '审阅项目结构'
    : language === 'en' ? 'Workspace configuration and docs' : '完善工作区配置与说明';
}

export function demoRun(base: ConversationRunVm, taskId: string, language: DesktopLanguage, runId = DEMO_RUN_ID) {
  const run = createPreviewRun(base, language === 'en' ? 'en' : 'zh');
  run.projectId = DEMO_PROJECT_ID;
  run.taskId = taskId;
  run.taskUuid = `demo-${taskId}`;
  run.runId = runId;
  run.runMode = demoRunMode(taskId);
  run.activeSessions = [];
  const session = run.selectedSession!;
  session.title = demoTitle(taskId, language);
  session.events = session.events.filter((event) => event.kind !== 'fileChangeSet');
  const en = language === 'en';
  session.events[0].content = en ? 'Inspect the workspace configuration and prepare a project guide.' : '请检查工作区配置，并整理一份项目说明。';
  session.events[1].content = en ? 'I will check the configuration defaults and summarize the project structure.' : '我会先检查配置默认值，再整理项目结构与审阅清单。';
  const tool = session.events.find((event) => event.kind === 'toolCall')!;
  tool.title = en ? 'Read workspace files' : '读取工作区文件';
  tool.raw = { toolCallId: tool.toolCallId, title: tool.title, status: 'completed' };
  if (taskId === 'demo-review') {
    session.events[0].content = en ? 'Review the project structure and summarize the responsibilities of each module.' : '请检查项目结构，整理各模块的职责和后续审阅重点。';
    session.events[1].content = en ? 'I will read the project notes and inspect the entry points.' : '我会先阅读项目说明，再检查各模块的入口与依赖关系。';
  }
  session.events.at(-1)!.content = en
    ? '## Review result\n\nThe workspace keeps configuration and application logic separate.\n\n- [README.md](/default/README.md): project notes\n- [src/config.json](/default/src/config.json): configuration\n\nThe next review can focus on input validation and regression coverage.'
    : '## 审阅结果\n\n工作区将配置与应用逻辑分开管理。\n\n- [README.md](/default/README.md)：项目说明\n- [src/config.json](/default/src/config.json)：配置内容\n\n后续可以重点检查输入校验和回归测试覆盖。';
  session.events.forEach((event, index) => { event.seq = index + 1; });
  session.eventPage = { loadedCount: session.events.length, total: session.events.length, oldestSeq: 1, newestSeq: session.events.length, hasOlder: false, hasNewer: false, oldestCursor: null, newestCursor: null };
  session.diagnostics.eventCount = session.events.length;
  if (run.runMode === 'workflow') {
    const template = mockWorkflowTemplates.templates.find((item) => item.id === 'default-lightweight')!;
    const profiles = demoProfiles(language);
    const seedRound = run.sessionTree.rounds[0];
    const seed = seedRound.nodes[0];
    const scenario = demoWorkflowRuns.find((scenario) => scenario.runId === runId)!;
    run.workflowTemplateId = template.id;
    run.workflowJson = JSON.stringify(template.workflow);
    run.workflowStatus = 'valid';
    run.sessionTree.rounds = DEMO_WORKFLOW_ROUNDS.map((roundId, index) => ({ ...seedRound, roundId, index: index + 1, label: roundId,
      nodes: template.workflow.nodes.map((node) => {
      const label = node.type === 'worker' ? profiles.find((profile) => profile.id === node.profile)?.name ?? node.id : node.id;
      const attempt = structuredClone(seed.attempts[0]);
      attempt.roundId = roundId;
      attempt.nodeId = node.id;
      attempt.pathLabel = `${node.id}/${attempt.attemptId}`;
      attempt.sessionId = `demo-${taskId}-${runId}-${roundId}-${node.id}`;
      attempt.current = index === DEMO_WORKFLOW_ROUNDS.length - 1 && node.id === 'accept';
      const startedAt = Date.parse(scenario.startedAt) + index * 6 * 60_000 + template.workflow.nodes.indexOf(node) * 2 * 60_000;
      attempt.startedAt = new Date(startedAt).toISOString();
      attempt.finishedAt = new Date(startedAt + 60_000).toISOString();
      attempt.attachmentCount = node.id === 'dev-test' ? demoDevelopmentFiles.attachments.length : 0;
      const needsRevision = index === 0 && node.id === 'accept';
      attempt.outcome = needsRevision ? 'failure' : 'success';
      const display: RuntimeDisplayVm = needsRevision ? { ...attempt.runtimeDisplay, code: 'failure', tone: 'danger', icon: 'error' } : attempt.runtimeDisplay;
      attempt.runtimeDisplay = display;
      if (attempt.lifecycle) {
        attempt.lifecycle.runtime.current = attempt.current;
        attempt.lifecycle.runtime.outcome = attempt.outcome;
        attempt.lifecycle.runtimeDisplay = display;
        attempt.lifecycle.displayStatus = needsRevision ? 'failure' : 'success';
        attempt.lifecycle.composer.lockInput = true;
      }
      return { ...seed, nodeId: node.id, label, runtimeDisplay: display, attempts: [attempt] };
    }) }));
    const round = run.sessionTree.rounds.at(-1)!;
    run.workflowGraph = {
      nodes: round.nodes.map((node) => ({ id: node.nodeId, nodeId: node.nodeId, label: node.label,
        nodeType: node.nodeType, status: 'completed', outcome: 'success', runtimeDisplay: node.runtimeDisplay,
        attemptId: node.attempts[0].attemptId, artifactCount: 0, attachmentCount: node.nodeId === 'dev-test' ? demoDevelopmentFiles.attachments.length : 0, current: false })),
      edges: template.workflow.edges.filter((edge) => !edge.to.startsWith('$')).map((edge) => ({ from: edge.from, to: edge.to, label: edge.on })),
    };
    run.selectedSession = demoSessionForNode(run, round.roundId, 'accept', language);
    run.sessionTree.selectedSessionKey = `${round.roundId}/accept/${round.nodes[0].attempts[0].attemptId}`;
  } else {
    run.selectedSession = demoSessionForNode(run, run.sessionTree.rounds[0].roundId, run.sessionTree.rounds[0].nodes[0].nodeId, language);
  }
  return run;
}

export function demoSessionForNode(run: ConversationRunVm, roundId: string, nodeId: string, language: DesktopLanguage) {
  const round = run.sessionTree.rounds.find((round) => round.roundId === roundId);
  const node = round?.nodes.find((node) => node.nodeId === nodeId);
  if (!node || !round) throw { code: 'demo.resource-not-found', params: { nodeId } };
  const session = structuredClone(run.selectedSession!);
  session.events = session.events.filter((event) => event.kind !== 'fileChangeSet');
  const leaf = node.attempts[0];
  Object.assign(session, { roundId: round.roundId, nodeId, attemptId: leaf.attemptId, sessionId: leaf.sessionId ?? session.sessionId });
  if (run.runMode === 'workflow') {
    session.sessionStartedAt = leaf.startedAt;
    session.sessionUpdatedAt = leaf.finishedAt;
    session.sessionElapsedSeconds = 60;
    session.timing = null;
    session.title = node.label;
    const en = language === 'en';
    const scenario = demoWorkflowRuns.find((scenario) => scenario.runId === run.runId)!;
    const goal = scenario.goals[language][round.index - 1];
    session.events[0].content = goal;
    session.events[1].content = en ? `I will work on: ${goal}.` : `本轮将完成：${goal}。`;
    const result: Record<string, string> = en ? {
      grill: 'The scope is agreed: review module boundaries, configuration defaults and the project guide.',
      'dev-test': 'The project guide and configuration checks are complete. Verification passed.\n\n[README.md](/default/README.md)',
      accept: '## Acceptance passed\n\nThe agreed checks are complete. The implementation and documentation are consistent.\n\n[README.md](/default/README.md)',
    } : {
      grill: '已明确审阅范围：模块边界、配置默认值和项目说明，后续按此范围执行。',
      'dev-test': '已完成项目说明整理与配置检查，验证通过。\n\n[README.md](/default/README.md)',
      accept: '## 验收通过\n\n约定的检查项已完成，实现与文档一致。\n\n[README.md](/default/README.md)',
    };
    session.events.at(-1)!.content = `${goal}\n\n${nodeId === 'accept' && round.index === 1
      ? en ? 'The initial review found missing coverage. Continue in the next round to address the findings.' : '首轮验收发现覆盖项不足，进入下一轮补充后重新验收。'
      : result[nodeId]}`;
    if (nodeId === 'dev-test') {
      session.events.splice(session.events.length - 1, 0, {
        id: 'dev-test-files', seq: 0, kind: 'fileChangeSet', status: 'finalized', timestamp: session.events[0].timestamp,
        raw: { changeSetId: demoDevelopmentFiles.id, summary: demoDevelopmentFiles.summary, attachmentCount: demoDevelopmentFiles.attachments.length },
      });
    }
    session.events.forEach((event, index) => { event.seq = index + 1; });
    session.events.forEach((event) => { event.id = `${run.runId}-${roundId}-${nodeId}-${event.seq}`; event.timestamp = leaf.startedAt!; });
    session.eventPage = { ...session.eventPage!, loadedCount: session.events.length, total: session.events.length, newestSeq: session.events.length };
    session.diagnostics.eventCount = session.events.length;
  }
  return session;
}

export function demoSidebar(language: DesktopLanguage): ConversationSidebarVm {
  const workflowRuns = demoWorkflowRuns.map((run) => ({ runId: run.runId, startedAt: run.startedAt, updatedAt: run.updatedAt, status: 'completed', outcome: 'success', resumable: false, currentRound: DEMO_WORKFLOW_ROUNDS.at(-1), currentNode: 'accept' }));
  const tasks: ConversationTaskRowVm[] = DEMO_TASKS.map((taskId) => ({
    projectId: DEMO_PROJECT_ID, taskId, taskUuid: `demo-${taskId}`,
    title: demoTitle(taskId, language), autoTitle: false, runMode: demoRunMode(taskId),
    agentIdentity: demoRunMode(taskId) === 'direct' ? (() => {
      const agent = demoAgentRegistry.agents.find((agent) => agent.agentType === 'claude-acp')!;
      return { agentType: agent.agentType, displayName: agent.displayName, iconKey: agent.iconKey };
    })() : null,
    latestRun: taskId === 'demo-review' ? workflowRuns[0] : { runId: DEMO_RUN_ID, status: 'completed', outcome: 'success', resumable: false, startedAt: '2026-09-01T08:00:00Z', updatedAt: '2026-09-01T08:01:00Z' },
    runs: taskId === 'demo-review' ? workflowRuns : [],
    runHistoryStatus: taskId === 'demo-review' ? 'ready' : 'ready-empty', runsNextCursor: null, pinned: false,
  }));
  return {
    loadStatus: 'ready', workspaces: [{ projectId: DEMO_PROJECT_ID, workspacePath: '/default', name: 'Gold Band' }],
    pinRefs: [], pinnedTasks: [], pinnedTaskPage: { status: 'ready-empty' },
    tasksByWorkspace: { [DEMO_PROJECT_ID]: tasks }, workspaceTaskPages: { [DEMO_PROJECT_ID]: { status: 'ready' } },
    lastActiveWorkspaceId: DEMO_PROJECT_ID, preferences: {},
  };
}
