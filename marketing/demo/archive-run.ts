import { z } from 'zod';
import type { ConversationRunVm, ConversationTreeNodeVm, ConversationSidebarVm } from '@/types';
import type { ArchiveCatalog } from './archive';
import { archiveTimestamp } from './archive-session';

export function withArchiveSidebar(sidebar: ConversationSidebarVm, catalog: ArchiveCatalog): ConversationSidebarVm {
  const startedAt = archiveTimestamp(catalog.run.startedAt);
  const updatedAt = archiveTimestamp(catalog.run.updatedAt);
  if (!startedAt || !updatedAt) throw { code: 'demo.archive-time-invalid', params: {} };
  const summary = { runId: catalog.run.id, status: catalog.run.status, outcome: catalog.run.outcome, resumable: false,
    startedAt, updatedAt };
  return { ...sidebar,
    workspaces: [...sidebar.workspaces, { projectId: catalog.projectId, workspacePath: `/archive/${catalog.projectId}`, name: catalog.projectId }],
    tasksByWorkspace: { ...sidebar.tasksByWorkspace, [catalog.projectId]: [{ projectId: catalog.projectId, taskId: catalog.task.id,
      taskUuid: typeof catalog.task.uuid === 'string' ? catalog.task.uuid : null, title: catalog.task.title, autoTitle: false,
      runMode: 'workflow', latestRun: summary, runs: [summary], runHistoryStatus: 'ready', runsNextCursor: null, pinned: false }] },
    workspaceTaskPages: { ...sidebar.workspaceTaskPages, [catalog.projectId]: { status: 'ready' } } };
}

const display = z.object({ code: z.string(), tone: z.string(), icon: z.string(), terminal: z.boolean(), resumable: z.boolean(), blockingError: z.boolean() }).passthrough();
const leaf = z.object({ roundId: z.string(), nodeId: z.string(), attemptId: z.string(),
  outerNodeId: z.string().optional(), outerAttemptId: z.string().optional(), pathLabel: z.string(), status: z.string(), outcome: z.string().nullable(),
  runtimeDisplay: display, current: z.boolean(), manualCheckPending: z.boolean(), artifactCount: z.number().int().nonnegative(), attachmentCount: z.number().int().nonnegative() }).passthrough();
const treeNode: z.ZodType<ConversationTreeNodeVm> = z.lazy(() => z.object({ nodeId: z.string(), label: z.string(), nodeType: z.string(),
  status: z.string(), runtimeDisplay: display, attempts: z.array(leaf), outerNodes: z.array(treeNode).optional() }));
export const archiveRunSchema: z.ZodType<ConversationRunVm> = z.object({
  projectId: z.string(), taskId: z.string(), runId: z.string(), runMode: z.enum(['direct', 'workflow', 'auto']), runStatus: z.string(), runOutcome: z.string().nullable(),
  sessionTree: z.object({ rounds: z.array(z.object({ roundId: z.string(), index: z.number().int(), label: z.string(), status: z.string(), runtimeDisplay: display, nodes: z.array(treeNode) })), selectedSessionKey: z.string().nullable() }),
  selectedSession: z.null(), activeSessions: z.array(z.never()), inputAttachments: z.array(z.never()),
  workflowStatus: z.string(), workflowValid: z.boolean(), resumable: z.boolean(),
  workflowGraph: z.object({ nodes: z.array(z.object({ id: z.string(), label: z.string(), nodeType: z.string(), runtimeDisplay: display,
    artifactCount: z.number().int(), attachmentCount: z.number().int(), current: z.boolean() }).passthrough()),
  edges: z.array(z.object({ from: z.string(), to: z.string(), label: z.string() }).passthrough()) }),
}).passthrough();
