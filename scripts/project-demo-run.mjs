const timestamp = value => {
  if (typeof value !== 'string') return null;
  const epoch = /^(\d+)Z$/.exec(value);
  const date = new Date(epoch ? Number(epoch[1]) * 1000 : value);
  return Number.isFinite(date.getTime()) ? date.toISOString() : null;
};

export function archiveDisplay(status, outcome, current = false, reason = null) {
  let code = outcome === 'success' ? 'success' : ['failure', 'failed', 'invalid'].includes(outcome) ? 'failure'
    : ['killed', 'cancelled', 'canceled'].includes(outcome) ? 'killed' : status ?? 'pending';
  if (code === 'paused' && current && ['error-blocked', 'runtime-abnormal'].includes(reason)) code = reason;
  const tone = code === 'success' ? 'success' : ['failure', 'killed', 'error-blocked', 'runtime-abnormal'].includes(code) ? 'danger'
    : code === 'running' ? 'running' : code === 'paused' ? 'warning' : 'neutral';
  return { code, tone, icon: code === 'success' ? 'check' : tone === 'danger' ? 'error' : code === 'paused' ? 'pause' : 'dot',
    terminal: ['success', 'failure', 'killed', 'completed'].includes(code), resumable: false, reasonCode: reason,
    blockingError: outcome === 'success' || ['failure', 'failed', 'invalid'].includes(outcome) ? false : ['failure', 'killed', 'error-blocked'].includes(code) };
}

// Mirrors view_models.rs::dynamic_graph_relations; a continuation is distinct from a structural edge.
export function archiveDynamicRelations(graph) {
  const ids = new Set(graph.nodes.map(node => node.id));
  const seen = new Set();
  const edges = [];
  const add = (from, to, label) => {
    const key = JSON.stringify([from, to, label === 'continue' ? 'continue' : 'structural']);
    if (from === to || !ids.has(from) || !ids.has(to) || seen.has(key)) return;
    seen.add(key);
    edges.push({ from, to, label, traversalCount: 1, lastOutcome: label === 'success' ? 'success' : null });
  };
  for (const node of graph.nodes) {
    for (const dependency of node.dependsOn ?? []) add(dependency, node.id, 'depends-on');
    if (node.sessionMode === 'continue') add(node.continueFromNodeId, node.id, 'continue');
  }
  for (const proposal of graph.proposals ?? []) {
    if (proposal.validationStatus !== 'accepted') continue;
    const next = proposal.parsed?.next;
    if (next?.type === 'single') add(proposal.sourceNodeId, next.node?.id, 'success');
    if (next?.type === 'fanout') for (const node of next.nodes ?? []) add(proposal.sourceNodeId, node.id, 'success');
  }
  for (const group of graph.groups ?? []) for (const root of group.rootNodeIds ?? []) add(group.createdByNodeId, root, 'success');
  return edges;
}

const locatorKey = locator => [locator.roundId, locator.outerNodeId, locator.outerAttemptId, locator.nodeId, locator.attemptId].filter(Boolean).join('/');
export function projectArchiveRun(manifest, rounds, dynamicGraphs) {
  const graphNodes = [];
  const graphEdges = [];
  const leaves = [];
  const dynamic = new Map(dynamicGraphs.map(entry => [JSON.stringify([entry.roundId, entry.nodeId, entry.attemptId]), entry.graph]));
  const sessionTree = { rounds: rounds.map(round => ({ roundId: round.id, index: round.index, label: round.id, status: round.status,
    runtimeDisplay: archiveDisplay(round.status, round.outcome), nodes: [] })), selectedSessionKey: null };
  for (const session of manifest.sessions) {
    const round = sessionTree.rounds.find(round => round.roundId === session.roundId);
    if (!round) throw new Error('archive.round-missing');
    const source = session.node;
    const graph = dynamic.get(JSON.stringify([session.roundId, session.outerNodeId, session.outerAttemptId]));
    const dynamicNode = graph?.nodes.find(node => node.id === session.nodeId);
    const current = manifest.run.current_round === session.roundId && (session.outerNodeId
      ? manifest.run.current_node === session.outerNodeId && graph?.run.currentNodeIds.includes(session.nodeId)
      : manifest.run.current_node === session.nodeId && manifest.run.current_attempt === session.attemptId);
    const display = archiveDisplay(source.status, source.outcome, current, current ? manifest.run.pause_reason : null);
    const leaf = { roundId: session.roundId, nodeId: session.nodeId, attemptId: session.attemptId,
      outerNodeId: session.outerNodeId, outerAttemptId: session.outerAttemptId, pathLabel: locatorKey(session),
      status: source.status, outcome: source.outcome, runtimeDisplay: display, current: Boolean(current), manualCheckPending: false,
      startedAt: timestamp(source.startedAt), finishedAt: timestamp(source.finishedAt), artifactCount: 0, attachmentCount: source.attachmentCount ?? 0 };
    leaves.push(leaf);
    let collection = round.nodes;
    if (session.outerNodeId) {
      let outer = collection.find(node => node.nodeId === session.outerNodeId);
      if (!outer) {
        if (!graph) throw new Error('archive.dynamic-graph-missing');
        outer = { nodeId: session.outerNodeId, label: session.outerNodeId, nodeType: 'ai-dynamic', status: graph.run.status,
          runtimeDisplay: archiveDisplay(graph.run.status, graph.run.outcome), attempts: [], outerNodes: [] };
        collection.push(outer);
      }
      collection = outer.outerNodes;
    }
    let node = collection.find(node => node.nodeId === session.nodeId);
    if (!node) {
      node = { nodeId: session.nodeId, label: source.title, nodeType: dynamicNode ? `dynamic-${dynamicNode.kind}` : 'worker', status: source.status, runtimeDisplay: display, attempts: [] };
      collection.push(node);
    }
    node.attempts.push(leaf);
  }
  for (const entry of dynamicGraphs) {
    if (entry.roundId !== manifest.run.current_round) continue;
    const id = nodeId => `${entry.nodeId}::${entry.attemptId}::${nodeId}`;
    for (const [index, node] of entry.graph.nodes.entries()) {
      const current = entry.graph.run.currentNodeIds.includes(node.id);
      const session = manifest.sessions.find(session => session.roundId === entry.roundId && session.outerNodeId === entry.nodeId
        && session.outerAttemptId === entry.attemptId && session.nodeId === node.id);
      graphNodes.push({ id: id(node.id), nodeId: node.id, label: node.title, nodeType: `dynamic-${node.kind}`, sequence: index + 1,
        status: node.status, outcome: node.outcome, runtimeDisplay: archiveDisplay(node.status, node.outcome, current, manifest.run.pause_reason),
        outerNodeId: entry.nodeId, outerAttemptId: entry.attemptId, attemptId: session?.attemptId ?? null,
        artifactCount: 0, attachmentCount: session?.node.attachmentCount ?? 0, current, sessionMode: node.sessionMode, continueFromNodeId: node.continueFromNodeId });
    }
    graphEdges.push(...archiveDynamicRelations(entry.graph).map(edge => ({ ...edge, from: id(edge.from), to: id(edge.to) })));
  }
  const selected = leaves.find(leaf => leaf.current) ?? leaves.toSorted((a, b) => (b.startedAt ?? '').localeCompare(a.startedAt ?? ''))[0];
  sessionTree.selectedSessionKey = selected ? locatorKey(selected) : null;
  return { projectId: manifest.projectId, taskId: manifest.task.id, taskUuid: manifest.task.uuid ?? null, runId: manifest.run.id,
    runMode: manifest.task.run_mode ?? 'workflow', runStatus: manifest.run.status, runOutcome: manifest.run.outcome ?? null,
    lastActivityAt: timestamp(manifest.run.updated_at), pauseReason: manifest.run.pause_reason ?? null, sessionTree,
    selectedSession: null, activeSessions: [], inputAttachments: [], workflowStatus: 'valid', workflowValid: true,
    workflowGraph: { nodes: graphNodes, edges: graphEdges }, resumable: false };
}
