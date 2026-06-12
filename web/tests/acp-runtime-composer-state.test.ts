import { describe, expect, it } from 'vitest';
import { deriveAcpRuntimeComposerState, type AcpRuntimeComposerStateInput } from '@/lib/acp-runtime-composer-state';
import type { ConversationAttemptLifecycleVm, RuntimeDisplayVm } from '@/types';

const pausedDisplay: RuntimeDisplayVm = {
  code: 'paused',
  tone: 'warning',
  icon: 'pause',
  terminal: false,
  resumable: true,
  reasonCode: 'process-interrupted',
};

const runningDisplay: RuntimeDisplayVm = {
  code: 'running',
  tone: 'running',
  icon: 'dot',
  terminal: false,
  resumable: false,
  reasonCode: null,
};

const completedDisplay: RuntimeDisplayVm = {
  code: 'completed',
  tone: 'neutral',
  icon: 'dot',
  terminal: true,
  resumable: false,
  reasonCode: null,
};

function lifecycle(overrides: Partial<ConversationAttemptLifecycleVm> = {}): ConversationAttemptLifecycleVm {
  return {
    runtime: {
      status: 'completed',
      outcome: null,
      pauseReason: null,
      resumable: false,
      current: false,
      active: false,
      continuable: false,
    },
    acp: {
      status: 'completed',
      active: false,
      stopping: false,
      terminal: true,
    },
    displayStatus: 'completed',
    runtimeDisplay: completedDisplay,
    continueKind: null,
    ...overrides,
  };
}

function baseInput(overrides: Partial<AcpRuntimeComposerStateInput> = {}): AcpRuntimeComposerStateInput {
  return {
    lifecycle: lifecycle(),
    legacyRuntimeStatus: 'completed',
    legacyRuntimeDisplay: completedDisplay,
    workflowValid: true,
    workflowInvalidMessage: 'Workflow invalid',
    pauseMessage: 'Paused',
    runtimeErrorMessage: null,
    acpStatus: 'completed',
    prompt: 'hello',
    waitingForPermission: false,
    hasPlanIntervention: false,
    sending: false,
    awaitingResponse: false,
    waitingForOptimisticPrompt: false,
    cancelling: false,
    stopCommandPending: false,
    turnAccepted: false,
    hasResponseAfterTurn: false,
    hasTimelineItems: true,
    hasEffectiveEvents: true,
    timelineProcessingKind: 'responding' as const,
    ...overrides,
  };
}

describe('deriveAcpRuntimeComposerState', () => {
  it('keeps stopping locked while ACP is cancelling', () => {
    const state = deriveAcpRuntimeComposerState(baseInput({
      lifecycle: lifecycle({
        runtime: {
          status: 'paused',
          outcome: null,
          pauseReason: 'process-interrupted',
          resumable: true,
          current: true,
          active: false,
          continuable: true,
        },
        acp: { status: 'cancelling', active: true, stopping: true, terminal: false },
        displayStatus: 'cancelling',
        runtimeDisplay: pausedDisplay,
        continueKind: 'input',
      }),
      acpStatus: 'cancelling',
    }));

    expect(state.mode).toBe('stopping');
    expect(state.stopInProgress).toBe(true);
    expect(state.inputDisabled).toBe(true);
    expect(state.canSubmit).toBe(false);
  });

  it('routes process-interrupted stopped input through runtime continue', () => {
    const state = deriveAcpRuntimeComposerState(baseInput({
      lifecycle: lifecycle({
        runtime: {
          status: 'paused',
          outcome: null,
          pauseReason: 'process-interrupted',
          resumable: true,
          current: true,
          active: false,
          continuable: true,
        },
        acp: { status: 'cancelled', active: false, stopping: false, terminal: true },
        displayStatus: 'paused',
        runtimeDisplay: pausedDisplay,
        continueKind: 'input',
      }),
      acpStatus: 'cancelled',
    }));

    expect(state.mode).toBe('interrupted-input');
    expect(state.submitTarget).toBe('runtime-continue');
    expect(state.inputDisabled).toBe(false);
    expect(state.canSubmit).toBe(true);
  });

  it('does not treat stale ACP cancelled as runtime error after continue starts', () => {
    const state = deriveAcpRuntimeComposerState(baseInput({
      lifecycle: lifecycle({
        runtime: {
          status: 'running',
          outcome: null,
          pauseReason: null,
          resumable: false,
          current: true,
          active: true,
          continuable: false,
        },
        acp: { status: 'cancelled', active: false, stopping: false, terminal: true },
        displayStatus: 'running',
        runtimeDisplay: runningDisplay,
      }),
      legacyRuntimeStatus: 'running',
      legacyRuntimeDisplay: runningDisplay,
      acpStatus: 'cancelled',
    }));

    expect(state.mode).toBe('runtime-active');
    expect(state.externalKind).toBeNull();
  });

  it('blocks waiting-for-user-input with an action instead of free ACP prompt', () => {
    const state = deriveAcpRuntimeComposerState(baseInput({
      lifecycle: lifecycle({
        runtime: {
          status: 'paused',
          outcome: null,
          pauseReason: 'waiting-for-user-input',
          resumable: true,
          current: true,
          active: false,
          continuable: true,
        },
        displayStatus: 'paused',
        runtimeDisplay: { ...pausedDisplay, reasonCode: 'waiting-for-user-input' },
        continueKind: 'action',
      }),
      legacyRuntimeDisplay: { ...pausedDisplay, reasonCode: 'waiting-for-user-input' },
    }));

    expect(state.mode).toBe('paused-action');
    expect(state.submitTarget).toBe('none');
    expect(state.inputDisabled).toBe(true);
    expect(state.showContinueAction).toBe(true);
  });

  it('only blocks invalid workflow on runtime continue paths', () => {
    const completed = deriveAcpRuntimeComposerState(baseInput({ workflowValid: false }));
    const interrupted = deriveAcpRuntimeComposerState(baseInput({
      workflowValid: false,
      lifecycle: lifecycle({
        runtime: {
          status: 'paused',
          outcome: null,
          pauseReason: 'process-interrupted',
          resumable: true,
          current: true,
          active: false,
          continuable: true,
        },
        displayStatus: 'paused',
        runtimeDisplay: pausedDisplay,
        continueKind: 'input',
      }),
      legacyRuntimeDisplay: pausedDisplay,
    }));

    expect(completed.mode).toBe('normal');
    expect(completed.submitTarget).toBe('acp-prompt');
    expect(interrupted.mode).toBe('invalid-workflow');
    expect(interrupted.submitTarget).toBe('none');
  });
});
