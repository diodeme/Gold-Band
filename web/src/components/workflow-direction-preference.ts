import { createContext, useContext } from 'react';
import { autoWorkflowLayoutDirection, type WorkflowLayoutDirection } from '@/components/workflowGraph';
import type { WorkflowGraphDirectionPreference } from '@/types';

export interface WorkflowGraphDirectionControl {
  preference: WorkflowGraphDirectionPreference;
  setPreference: (next: WorkflowGraphDirectionPreference) => void;
}

const fallbackControl: WorkflowGraphDirectionControl = {
  preference: 'auto',
  setPreference: () => {},
};

const WorkflowGraphDirectionContext = createContext<WorkflowGraphDirectionControl | null>(null);

export const WorkflowGraphDirectionProvider = WorkflowGraphDirectionContext.Provider;

/**
 * Direction preference shared by every workflow graph surface (run graph,
 * authoring editor, fullscreen view). Falls back to `auto` without a
 * provider so isolated tests and embeds keep rendering.
 */
export function useWorkflowGraphDirectionPreference(): WorkflowGraphDirectionControl {
  return useContext(WorkflowGraphDirectionContext) ?? fallbackControl;
}

/** Maps the user preference to the effective ELK direction for one container. */
export function resolveWorkflowLayoutDirection(
  preference: WorkflowGraphDirectionPreference,
  width: number,
  height: number,
  previous?: WorkflowLayoutDirection,
): WorkflowLayoutDirection {
  if (preference === 'vertical') return 'DOWN';
  if (preference === 'horizontal') return 'RIGHT';
  return autoWorkflowLayoutDirection(width, height, previous);
}
