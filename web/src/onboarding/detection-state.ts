import type { AppErrorVm } from '@/types';
import type { EnvironmentCheck, EnvironmentReport, SetupAgent, SetupEvent, SetupUnavailableAgent } from './types';
export interface SetupDetectionState {
  environment: { phase: 'idle' | 'checking' | 'ready' | 'failed'; report?: EnvironmentReport; progress: Partial<Record<EnvironmentCheck['tool'], EnvironmentCheck>> };
  phase: 'idle' | 'detecting' | 'completed' | 'failed';
  generation: string;
  agents: Record<string, SetupAgent>;
  unavailable: Record<string, SetupUnavailableAgent>;
  pending: Record<string, string>;
  nextCursor: string | null;
}
export const initialDetectionState: SetupDetectionState = {
  environment: { phase: 'checking', progress: {} }, phase: 'idle', generation: '',
  agents: {}, unavailable: {}, pending: {}, nextCursor: null,
};
type Action =
  | { type: 'environment-started' }
  | { type: 'environment-progress'; check: EnvironmentCheck }
  | { type: 'environment-completed'; report: EnvironmentReport }
  | { type: 'environment-failed' }
  | { type: 'detection-started'; append: boolean }
  | { type: 'detection-stopped' }
  | { type: 'detection-accepted'; generation: string }
  | { type: 'detection-failed' }
  | { type: 'event'; event: SetupEvent }
  | { type: 'model'; agent: SetupAgent };

export function setupFailure(error: unknown): AppErrorVm {
  if (error && typeof error === 'object' && 'code' in error && typeof error.code === 'string') {
    return { code: error.code, params: 'params' in error && error.params && typeof error.params === 'object' ? error.params as Record<string, unknown> : {} };
  }
  return { code: 'setup.probe-failed', params: {} };
}

function without<T>(record: Record<string, T>, id: string): Record<string, T> {
  if (!(id in record)) return record;
  const next = { ...record }; delete next[id];
  return next;
}

export function setupDetectionReducer(state: SetupDetectionState, action: Action): SetupDetectionState {
  switch (action.type) {
    case 'environment-started': return { ...state, phase: 'idle', generation: '', pending: {}, nextCursor: null, environment: { ...state.environment, phase: 'checking', progress: {} } };
    case 'environment-progress': return state.environment.phase === 'checking' ? { ...state, environment: { ...state.environment, progress: { ...state.environment.progress, [action.check.tool]: action.check } } } : state;
    case 'environment-completed': return { ...state, phase: 'idle', environment: { phase: 'ready', report: action.report, progress: {} } };
    case 'environment-failed': return { ...state, environment: { phase: 'failed', progress: {} } };
    case 'detection-started': return { ...state, phase: 'detecting', generation: '', agents: action.append ? state.agents : {}, unavailable: action.append ? state.unavailable : {}, pending: {}, nextCursor: null };
    case 'detection-stopped': return { ...state, phase: 'idle', generation: '', pending: {} };
    case 'detection-accepted': return state.phase === 'detecting' || state.phase === 'completed' ? { ...state, generation: action.generation } : state;
    case 'detection-failed': return { ...state, phase: 'failed', generation: '', pending: {} };
    case 'model': return { ...state, agents: { ...state.agents, [action.agent.agentType]: action.agent } };
    case 'event': {
      const event = action.event;
      if ((state.phase !== 'detecting' && state.phase !== 'completed') || (state.generation && state.generation !== event.generation)) return state;
      const current = { ...state, generation: event.generation };
      if (event.type === 'failed') return setupDetectionReducer(current, { type: 'detection-failed' });
      if (event.type === 'checking') return { ...current, pending: { ...state.pending, [event.agentId]: event.label } };
      if (event.type === 'finished') return { ...current, phase: 'completed', pending: {}, nextCursor: event.nextCursor };
      const id = event.type === 'ready' ? event.agent.agentType : event.agentId;
      const settled = { ...current, pending: without(state.pending, id), agents: without(state.agents, id), unavailable: without(state.unavailable, id) };
      if (event.type === 'ready') return { ...settled, agents: { ...settled.agents, [id]: event.agent } };
      if (event.type === 'unavailable' && event.reason.kind !== 'not-installed') {
        return { ...settled, unavailable: { ...settled.unavailable, [id]: { agentType: id, displayName: event.label, iconKey: event.iconKey, reason: event.reason } } };
      }
      return settled;
    }
  }
}
