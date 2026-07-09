import type { WorkflowDsl } from '../types';

/**
 * Removes the interview node and its edges from a workflow, fixing the entry
 * point to "plan" if the original entry was "interview". Mirrors the backend
 * `strip_interview_node` function. This is a pure, display-only transform.
 */
export function stripInterviewNode(workflow: WorkflowDsl): WorkflowDsl {
  const hasInterview = workflow.nodes.some((n) => n.id === 'interview');
  if (!hasInterview) return workflow;
  return {
    ...workflow,
    entry: workflow.entry === 'interview' ? 'plan' : workflow.entry,
    nodes: workflow.nodes.filter((n) => n.id !== 'interview'),
    edges: workflow.edges.filter((e) => e.from !== 'interview' && e.to !== 'interview'),
  };
}

/**
 * Re-inserts the interview node (and its edges) from `source` into `target`
 * if the source workflow had an interview node. Used to preserve the interview
 * node in the underlying template data while hiding it from the canvas.
 */
export function mergeInterviewNode(source: WorkflowDsl, target: WorkflowDsl): WorkflowDsl {
  const interviewNode = source.nodes.find((n) => n.id === 'interview');
  if (!interviewNode) return target;
  const interviewEdges = source.edges.filter((e) => e.from === 'interview' || e.to === 'interview');
  const targetHasInterview = target.nodes.some((n) => n.id === 'interview');
  if (targetHasInterview) return target;
  return {
    ...target,
    entry: source.entry === 'interview' ? 'interview' : target.entry,
    nodes: [interviewNode, ...target.nodes],
    edges: [...interviewEdges, ...target.edges],
  };
}
