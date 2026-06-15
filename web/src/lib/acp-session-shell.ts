export interface AcpLiveSessionShellPolicyInput {
  runtimeActive: boolean;
  allowEventOnlySessionShell: boolean;
  loadedEventCount: number;
}

export type AcpSessionShellState = 'available' | 'loading' | 'missing';

export interface AcpSessionShellStateInput {
  hasBaseSession: boolean;
  hasLiveSessionShell: boolean;
  initialSessionLoading: boolean;
}

export function shouldCreateLiveAcpSessionShell(input: AcpLiveSessionShellPolicyInput) {
  if (input.runtimeActive) return true;
  return input.allowEventOnlySessionShell && input.loadedEventCount > 0;
}

export function resolveAcpSessionShellState(input: AcpSessionShellStateInput): AcpSessionShellState {
  if (input.hasBaseSession || input.hasLiveSessionShell) return 'available';
  if (input.initialSessionLoading) return 'loading';
  return 'missing';
}
