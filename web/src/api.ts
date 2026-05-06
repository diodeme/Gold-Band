import { invoke } from '@tauri-apps/api/core';
import {
  mockBootstrap,
  mockContent,
  mockRoundDetail,
  mockRunDetail,
  mockTaskDetail,
  mockTaskList,
  mockWorkflow,
} from './mockData';
import type {
  AppBootstrapVm,
  ContentVm,
  DesktopLanguage,
  DesktopThemePreference,
  PreferencesVm,
  RoundDetailVm,
  RoundSelection,
  RunDetailVm,
  RunSummaryVm,
  TaskDetailVm,
  TaskListVm,
  WorkflowVm,
} from './types';

const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

function command<T>(name: string, args?: Record<string, unknown>, fallback?: T): Promise<T> {
  if (!isTauri && fallback !== undefined) {
    return Promise.resolve(fallback);
  }
  return invoke<T>(name, args);
}

export function getAppBootstrap() {
  return command<AppBootstrapVm>('get_app_bootstrap', undefined, mockBootstrap);
}

export function getTaskList() {
  return command<TaskListVm>('get_task_list', undefined, mockTaskList);
}

export function chooseWorkspace() {
  return command<AppBootstrapVm | null>('choose_workspace', undefined, mockBootstrap);
}

export function selectRecentWorkspace(workspace: string) {
  return command<AppBootstrapVm>('select_recent_workspace', { workspace }, { ...mockBootstrap, repoRoot: workspace });
}

export function getTaskDetail(taskId: string) {
  return command<TaskDetailVm>('get_task_detail', { taskId }, { ...mockTaskDetail, task: mockTaskList.tasks.find((item) => item.id === taskId) ?? mockTaskDetail.task });
}

export function getWorkflow(taskId: string) {
  return command<WorkflowVm>('get_workflow', { taskId }, { ...mockWorkflow, task: mockTaskList.tasks.find((item) => item.id === taskId) ?? mockWorkflow.task });
}

export function getRunDetail(taskId: string, runId: string) {
  return command<RunDetailVm>('get_run_detail', { taskId, runId }, { ...mockRunDetail, run: { ...mockRunDetail.run, id: runId, taskId } });
}

export function getRoundDetail(taskId: string, runId: string, roundId: string, selection?: RoundSelection) {
  return command<RoundDetailVm>('get_round_detail', { taskId, runId, roundId, selection: toRoundSelectionInput(selection) }, mockRoundDetail(selection));
}

function toRoundSelectionInput(selection?: RoundSelection) {
  if (!selection) return selection;
  if (selection.kind === 'round' || selection.kind === 'requirement') return { kind: selection.kind, context_node_id: selection.contextNodeId };
  if (selection.kind === 'event' || selection.kind === 'log') return { kind: selection.kind, id: selection.id, node_id: selection.nodeId, attempt_id: selection.attemptId, context_node_id: selection.contextNodeId };
  if (selection.kind === 'node') return { kind: selection.kind, node_id: selection.nodeId, context_node_id: selection.contextNodeId };
  if (selection.kind === 'worker-ref') return { kind: selection.kind, node_id: selection.nodeId, attempt_id: selection.attemptId, context_node_id: selection.contextNodeId };
  return { kind: selection.kind, node_id: selection.nodeId, attempt_id: selection.attemptId, name: selection.name, context_node_id: selection.contextNodeId };
}

export function startRun(taskId: string) {
  return command<RunSummaryVm>('start_run', { taskId }, { ...mockRunDetail.run, taskId });
}

export function continueRun(taskId: string, runId: string) {
  return command<RunSummaryVm>('continue_run', { taskId, runId }, { ...mockRunDetail.run, taskId, id: runId });
}

export function retryRun(taskId: string, runId: string) {
  return command<RunSummaryVm>('retry_run', { taskId, runId }, { ...mockRunDetail.run, taskId, id: runId });
}

export function killRun(taskId: string, runId: string) {
  return command<RunSummaryVm>('kill_run', { taskId, runId }, { ...mockRunDetail.run, taskId, id: runId, status: 'completed', outcome: 'killed' });
}

export function showArtifact(taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, name: string) {
  return command<ContentVm>('show_artifact', { taskId, runId, roundId, nodeId, attemptId, name }, { ...mockContent, title: name });
}

export function showAttachment(taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, name: string) {
  return command<ContentVm>('show_attachment', { taskId, runId, roundId, nodeId, attemptId, name }, { ...mockContent, title: name, kind: 'attachment' });
}

export function showWorkerRef(taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string) {
  return command<ContentVm>('show_worker_ref', { taskId, runId, roundId, nodeId, attemptId }, { ...mockContent, title: attemptId, kind: 'worker-ref' });
}

export function saveDesktopPreferences(theme: DesktopThemePreference, language: DesktopLanguage) {
  return command<PreferencesVm>('save_desktop_preferences', { theme, language }, { theme, language });
}
