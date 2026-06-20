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
  blockingError: false,
};

const runningDisplay: RuntimeDisplayVm = {
  code: 'running',
  tone: 'running',
  icon: 'dot',
  terminal: false,
  resumable: false,
  reasonCode: null,
  blockingError: false,
};

const completedDisplay: RuntimeDisplayVm = {
  code: 'completed',
  tone: 'neutral',
  icon: 'dot',
  terminal: true,
  resumable: false,
  reasonCode: null,
  blockingError: false,
};

const workflowFailureDisplay: RuntimeDisplayVm = {
  code: 'failure',
  tone: 'danger',
  icon: 'error',
  terminal: true,
  resumable: false,
  reasonCode: null,
  blockingError: false,
};

type LifecycleOverrides = Omit<Partial<ConversationAttemptLifecycleVm>, 'runtime' | 'acp' | 'composer'> & {
  runtime?: Partial<ConversationAttemptLifecycleVm['runtime']>;
  acp?: Partial<ConversationAttemptLifecycleVm['acp']>;
  composer?: Partial<ConversationAttemptLifecycleVm['composer']>;
};

function lifecycle(overrides: LifecycleOverrides = {}): ConversationAttemptLifecycleVm {
  const base: ConversationAttemptLifecycleVm = {
    runtime: {
      status: 'completed',
      outcome: null,
      pauseReason: null,
      resumable: false,
      current: false,
      active: false,
      continuable: false,
      phase: 'terminal',
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
    composer: {
      mode: 'normal',
      submitTarget: 'acp-prompt',
      processingKind: 'processing',
      statusKey: null,
      canStop: false,
      lockInput: false,
      showContinueAction: false,
    },
  };
  const merged = {
    ...base,
    ...overrides,
    runtime: { ...base.runtime, ...overrides.runtime },
    acp: { ...base.acp, ...overrides.acp },
    composer: { ...base.composer, ...overrides.composer },
  };
  if (!overrides.composer) {
    if (merged.acp.stopping) {
      merged.composer = { ...merged.composer, mode: 'stopping', submitTarget: 'none', processingKind: 'stopping', canStop: true, lockInput: true };
    } else if (merged.runtime.active || merged.acp.active) {
      merged.composer = {
        ...merged.composer,
        mode: 'runtime-active',
        submitTarget: 'none',
        processingKind: merged.runtime.phase === 'launching-next-node' ? 'launching-next-node' : 'processing',
        canStop: true,
        lockInput: true,
      };
    } else if (merged.continueKind === 'input') {
      merged.composer = { ...merged.composer, mode: 'interrupted-input', submitTarget: 'runtime-continue', lockInput: false };
    } else if (merged.continueKind === 'action') {
      merged.composer = { ...merged.composer, mode: 'paused-action', submitTarget: 'runtime-continue', lockInput: true, showContinueAction: true };
    }
  }
  return merged;
}

function baseInput(overrides: Partial<AcpRuntimeComposerStateInput> = {}): AcpRuntimeComposerStateInput {
  return {
    lifecycle: lifecycle(),
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
      acpStatus: 'cancelled',
    }));

    expect(state.mode).toBe('runtime-active');
    expect(state.externalKind).toBeNull();
  });

  it('uses backend lifecycle after terminal ACP snapshots finish stopping', () => {
    for (const acpStatus of ['cancelled', 'canceled', 'failed', 'failure', 'error', 'killed']) {
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
            phase: 'paused',
          },
          acp: { status: acpStatus, active: false, stopping: false, terminal: true },
          displayStatus: 'paused',
          runtimeDisplay: pausedDisplay,
          continueKind: 'input',
        }),
        acpStatus,
        cancelling: true,
        stopCommandPending: true,
        awaitingResponse: true,
        turnAccepted: true,
        hasResponseAfterTurn: false,
      }));

      expect(state.mode).toBe('interrupted-input');
      expect(state.stopInProgress).toBe(false);
      expect(state.sessionActive).toBe(false);
      expect(state.statusActive).toBe(false);
      expect(state.processingKind).toBe('responding');
      expect(state.submitTarget).toBe('runtime-continue');
      expect(state.inputDisabled).toBe(false);
      expect(state.canStop).toBe(false);
    }
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
    }));

    expect(state.mode).toBe('paused-action');
    expect(state.submitTarget).toBe('runtime-continue');
    expect(state.inputDisabled).toBe(true);
    expect(state.showContinueAction).toBe(true);
  });

  it('does not turn workflow outcome failure into runtime error', () => {
    const state = deriveAcpRuntimeComposerState(baseInput({
      lifecycle: lifecycle({
        runtime: {
          status: 'completed',
          outcome: 'failure',
          pauseReason: null,
          resumable: false,
          current: false,
          active: false,
          continuable: false,
        },
        displayStatus: 'completed',
        runtimeDisplay: workflowFailureDisplay,
      }),
    }));

    expect(state.mode).toBe('normal');
    expect(state.externalKind).toBeNull();
    expect(state.inputDisabled).toBe(false);
  });

  it('ignores stale awaiting response when lifecycle is terminal', () => {
    const state = deriveAcpRuntimeComposerState(baseInput({
      awaitingResponse: true,
      turnAccepted: true,
      hasResponseAfterTurn: false,
      acpStatus: 'completed',
      hasTimelineItems: true,
      hasEffectiveEvents: true,
      timelineProcessingKind: 'responding',
    }));

    expect(state.mode).toBe('normal');
    expect(state.sessionActive).toBe(false);
    expect(state.statusActive).toBe(false);
    expect(state.processingKind).toBe('responding');
    expect(state.canStop).toBe(false);
  });

  it('ignores stale ACP running when lifecycle is terminal', () => {
    const state = deriveAcpRuntimeComposerState(baseInput({
      acpStatus: 'running',
      hasTimelineItems: true,
      hasEffectiveEvents: true,
      timelineProcessingKind: 'responding',
    }));

    expect(state.mode).toBe('normal');
    expect(state.sessionActive).toBe(false);
    expect(state.acpActive).toBe(false);
    expect(state.statusActive).toBe(false);
  });

  it('keeps backend launching-next-node active after ACP completes', () => {
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
          phase: 'launching-next-node',
        },
        acp: { status: 'completed', active: false, stopping: false, terminal: true },
        displayStatus: 'running',
        runtimeDisplay: runningDisplay,
      }),
      acpStatus: 'completed',
      awaitingResponse: true,
      turnAccepted: true,
      hasResponseAfterTurn: false,
    }));

    expect(state.mode).toBe('runtime-active');
    expect(state.runtimeActive).toBe(true);
    expect(state.sessionActive).toBe(true);
    expect(state.statusActive).toBe(true);
    expect(state.processingKind).toBe('launching-next-node');
    expect(state.canStop).toBe(true);
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
    }));

    expect(completed.mode).toBe('normal');
    expect(completed.submitTarget).toBe('acp-prompt');
    expect(interrupted.mode).toBe('invalid-workflow');
    expect(interrupted.submitTarget).toBe('none');
  });
});
