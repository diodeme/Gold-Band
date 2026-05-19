import type { TFunction } from 'i18next';
import type { GraphNodeVm, GraphVm } from '../types';
import { displayNodeType } from '../i18n';

export function formatCurrentNode(t: TFunction, graph: GraphVm, nodeId?: string | null) {
  if (!nodeId) return '-';
  const node = findGraphNode(graph, nodeId);
  if (!node) return formatUnknownNode(t, nodeId);
  return formatGraphNode(t, node);
}

export function findGraphNode(graph: GraphVm, nodeId: string) {
  return graph.nodes.find((node) => node.nodeId === nodeId || node.id === nodeId);
}

function formatGraphNode(t: TFunction, node: GraphNodeVm) {
  const id = node.nodeId ?? node.id;
  const label = node.label.trim();
  const type = displayNodeType(t, node.nodeType);
  if (label && label !== id) return t('node.summaryWithLabel', { type, label, id });
  return t('node.summary', { type, id });
}

function formatUnknownNode(t: TFunction, id: string) {
  const type = displayNodeType(t, inferNodeType(id));
  const label = t(`node.fallbackLabels.${id}`, { defaultValue: humanizeNodeId(id) });
  return t('node.summaryWithLabel', { type, label, id });
}

function inferNodeType(id: string) {
  const normalized = id.toLowerCase();
  if (/^(run|exec|test|tests|build|lint|format|check)(-|_|$)/.test(normalized) || normalized.includes('test')) return 'exec';
  if (normalized.includes('dev') || normalized.includes('plan') || normalized.includes('worker') || normalized.includes('accept') || normalized.includes('review') || normalized.includes('validate')) return 'worker';
  return 'unknown';
}

function humanizeNodeId(id: string) {
  return id.replace(/[-_]+/g, ' ');
}
