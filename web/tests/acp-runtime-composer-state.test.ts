import { describe, expect, it } from 'vitest';
import {
  deriveAcpRuntimeComposerState,
  mergeConversationAttemptLifecycle,
  shouldKeepLocalRuntimeLifecycleOverride,
  type AcpRuntimeComposerStateInput,
} from '@/lib/acp-runtime-composer-state';
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

const runtimeAbnormalDisplay: RuntimeDisplayVm = {
  code: 'runtime-abnormal',
  tone: 'danger',
  icon: 'error',
  terminal: false,
  resumable: true,
  reasonCode: 'runtime-abnormal',
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
  it('keeps the Direct composer editable and queues submissions while a turn is active', () => {
    const state = deriveAcpRuntimeComposerState(baseInput({
      lifecycle: lifecycle({
        runtime: { status: 'running', active: true, current: true, phase: 'provider-running' },
        acp: { status: 'running', active: true, terminal: false },
        composer: {
          mode: 'runtime-active',
          submitTarget: 'queue-prompt',
          lockInput: false,
          canStop: true,
        },
        promptQueue: { revision: 0, items: [], maxItems: 10 },
      }),
      acpStatus: 'running',
    }));

    expect(state.submitTarget).toBe('queue-prompt');
    expect(state.inputDisabled).toBe(false);
    expect(state.canSubmit).toBe(true);
    expect(state.placeholderKind).toBe('default');
  });

  it('keeps Direct input focusable through the initial sending transition', () => {
    const initialSending = deriveAcpRuntimeComposerState(baseInput({
      promptQueueEnabled: true,
      lifecycle: lifecycle({
        continueKind: 'input',
        composer: { mode: 'interrupted-input', submitTarget: 'runtime-continue', lockInput: false },
      }),
      sending: true,
      prompt: 'next prompt',
    }));
    const runningSending = deriveAcpRuntimeComposerState(baseInput({
      lifecycle: lifecycle({
        runtime: { status: 'running', active: true, current: true, phase: 'provider-running' },
        acp: { status: 'running', active: true, terminal: false },
        composer: { mode: 'runtime-active', submitTarget: 'queue-prompt', lockInput: false },
        promptQueue: { revision: 0, items: [], maxItems: 10 },
      }),
      acpStatus: 'running',
      sending: true,
      prompt: 'next prompt',
    }));

    expect(initialSending.inputDisabled).toBe(false);
    expect(initialSending.submitTarget).toBe('queue-prompt');
    expect(initialSending.canSubmit).toBe(true);
    expect(runningSending.inputDisabled).toBe(false);
    expect(runningSending.canSubmit).toBe(true);
  });

  it('rejects another queued submission when the Direct queue reaches capacity', () => {
    const items = Array.from({ length: 10 }, (_, index) => ({
      id: `queued-${index}`,
      content: `prompt ${index}`,
      attachmentCount: 0,
      createdAt: '2026-08-07T00:00:00Z',
    }));
    const state = deriveAcpRuntimeComposerState(baseInput({
      lifecycle: lifecycle({
        runtime: { status: 'running', active: true, current: true, phase: 'provider-running' },
        acp: { status: 'running', active: true, terminal: false },
        composer: {
          mode: 'runtime-active',
          submitTarget: 'queue-prompt',
          lockInput: false,
          canStop: true,
        },
        promptQueue: { revision: 10, items, maxItems: 10 },
      }),
      acpStatus: 'running',
    }));

    expect(state.inputDisabled).toBe(false);
    expect(state.canSubmit).toBe(false);
  });

  it('does not unlock a non-Direct active composer without the backend queue target', () => {
    const state = deriveAcpRuntimeComposerState(baseInput({
      lifecycle: lifecycle({
        runtime: { status: 'running', active: true, current: true, phase: 'provider-running' },
        acp: { status: 'running', active: true, terminal: false },
      }),
      acpStatus: 'running',
    }));

    expect(state.submitTarget).toBe('none');
    expect(state.inputDisabled).toBe(true);
    expect(state.canSubmit).toBe(false);
  });

  it('restores a completed-run live follow-up from backend lifecycle without optimistic state', () => {
    const state = deriveAcpRuntimeComposerState(baseInput({
      lifecycle: lifecycle({
        runtime: {
          status: 'completed',
          outcome: 'success',
          pauseReason: null,
          resumable: false,
          current: false,
          active: false,
          continuable: false,
          phase: 'provider-running',
        },
        acp: { status: 'running', phase: 'running', active: true, stopping: false, terminal: false },
      }),
      acpStatus: 'running',
      sending: false,
      awaitingResponse: false,
      waitingForOptimisticPrompt: false,
    }));

    expect(state.mode).toBe('runtime-active');
    expect(state.inputDisabled).toBe(true);
    expect(state.canStop).toBe(true);
    expect(state.showStatus).toBe(true);
  });

  it('keeps stop available when a previous terminal snapshot races a live follow-up', () => {
    const state = deriveAcpRuntimeComposerState(baseInput({
      lifecycle: lifecycle({
        runtime: {
          status: 'completed',
          outcome: 'success',
          active: false,
          phase: 'provider-running',
        },
        acp: {
          status: 'running',
          phase: 'running',
          active: true,
          stopping: false,
          terminal: false,
        },
      }),
      acpStatus: 'completed',
      localTurnInFlight: true,
      awaitingResponse: false,
      sending: false,
      waitingForOptimisticPrompt: false,
      timelineProcessingKind: 'tool',
    }));

    expect(state.acpActive).toBe(true);
    expect(state.sessionActive).toBe(true);
    expect(state.statusActive).toBe(true);
    expect(state.processingKind).toBe('tool');
    expect(state.canStop).toBe(true);
    expect(state.inputDisabled).toBe(true);
  });

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

  it('lets a terminal ACP snapshot override a stale cancelling lifecycle', () => {
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
      acpStatus: 'cancelled',
      cancelling: true,
      stopCommandPending: true,
    }));

    expect(state.stopInProgress).toBe(false);
    expect(state.mode).toBe('interrupted-input');
    expect(state.inputDisabled).toBe(false);
    expect(state.canStop).toBe(false);
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

  it('routes runtime-abnormal stopped input through runtime continue', () => {
    const state = deriveAcpRuntimeComposerState(baseInput({
      lifecycle: lifecycle({
        runtime: {
          status: 'paused',
          outcome: null,
          pauseReason: 'runtime-abnormal',
          resumable: true,
          current: true,
          active: false,
          continuable: true,
        },
        acp: { status: 'cancelled', active: false, stopping: false, terminal: true },
        displayStatus: 'runtime-abnormal',
        runtimeDisplay: runtimeAbnormalDisplay,
        continueKind: 'input',
      }),
      acpStatus: 'cancelled',
    }));

    expect(state.mode).toBe('interrupted-input');
    expect(state.submitTarget).toBe('runtime-continue');
    expect(state.inputDisabled).toBe(false);
    expect(state.canSubmit).toBe(true);
  });

  it('ignores stale runtime error messages unless backend composer is runtime-error', () => {
    const activeState = deriveAcpRuntimeComposerState(baseInput({
      lifecycle: lifecycle({
        runtime: {
          status: 'paused',
          outcome: null,
          pauseReason: null,
          resumable: false,
          current: true,
          active: true,
          continuable: false,
          phase: 'provider-running',
        },
        acp: { status: 'failed', active: false, stopping: false, terminal: true },
        displayStatus: 'paused',
        runtimeDisplay: pausedDisplay,
        composer: {
          mode: 'runtime-active',
          submitTarget: 'none',
          processingKind: 'processing',
          statusKey: 'conversation.runtime.runtimeActive',
          canStop: true,
          lockInput: true,
        },
      }),
      acpStatus: 'failed',
      runtimeErrorMessage: '当前会话运行失败，请查看错误原因',
    }));

    expect(activeState.mode).toBe('runtime-active');
    expect(activeState.externalKind).toBeNull();

    const abnormalState = deriveAcpRuntimeComposerState(baseInput({
      lifecycle: lifecycle({
        runtime: {
          status: 'paused',
          outcome: null,
          pauseReason: 'runtime-abnormal',
          resumable: true,
          current: true,
          active: false,
          continuable: true,
        },
        acp: { status: 'failed', active: false, stopping: false, terminal: true },
        displayStatus: 'runtime-abnormal',
        runtimeDisplay: runtimeAbnormalDisplay,
        continueKind: 'input',
      }),
      acpStatus: 'failed',
      runtimeErrorMessage: '当前会话运行失败，请查看错误原因',
    }));

    expect(abnormalState.mode).toBe('interrupted-input');
    expect(abnormalState.submitTarget).toBe('runtime-continue');
    expect(abnormalState.externalKind).toBeNull();
    expect(abnormalState.inputDisabled).toBe(false);
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

  it('ignores stale optimistic sending state once ACP has reached terminal paused lifecycle', () => {
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
        acp: { status: 'cancelled', active: false, stopping: false, terminal: true },
        displayStatus: 'paused',
        runtimeDisplay: pausedDisplay,
        continueKind: 'input',
      }),
      acpStatus: 'cancelled',
      waitingForOptimisticPrompt: true,
      awaitingResponse: true,
      turnAccepted: false,
      hasResponseAfterTurn: false,
    }));

    expect(state.mode).toBe('interrupted-input');
    expect(state.processingKind).toBe('responding');
    expect(state.statusActive).toBe(false);
    expect(state.inputDisabled).toBe(false);
    expect(state.canStop).toBe(false);
    expect(state.canSubmit).toBe(true);
  });

  it('surfaces a typed compacting phase from the latest timeline event', () => {
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
          phase: 'running',
        },
        acp: { status: 'running', active: true, stopping: false, terminal: false },
        displayStatus: 'running',
        runtimeDisplay: runningDisplay,
      }),
      hasTimelineItems: true,
      hasEffectiveEvents: true,
      timelineProcessingKind: 'compacting',
    }));

    expect(state.processingKind).toBe('compacting');
    expect(state.statusActive).toBe(true);
    expect(state.canStop).toBe(true);
  });

  it('keeps manual-check waiting state available for regular ACP prompts', () => {
    const state = deriveAcpRuntimeComposerState(baseInput({
      lifecycle: lifecycle({
        runtime: {
          status: 'paused',
          outcome: null,
          pauseReason: 'waiting-for-user-input',
          resumable: true,
          current: true,
          active: false,
          continuable: false,
          phase: 'paused',
        },
        displayStatus: 'paused',
        runtimeDisplay: { ...pausedDisplay, reasonCode: 'waiting-for-user-input' },
        continueKind: null,
        composer: {
          mode: 'normal',
          submitTarget: 'acp-prompt',
          processingKind: 'processing',
          statusKey: null,
          canStop: false,
          lockInput: false,
        },
      }),
    }));

    expect(state.mode).toBe('normal');
    expect(state.submitTarget).toBe('acp-prompt');
    expect(state.inputDisabled).toBe(false);
    expect(state.showExternalState).toBe(false);
    expect(state.canSubmit).toBe(true);
  });

  it('keeps permission waits as a locked runtime composer state', () => {
    const state = deriveAcpRuntimeComposerState(baseInput({
      lifecycle: lifecycle(),
      waitingForPermission: true,
      prompt: 'allow?',
    }));

    expect(state.mode).toBe('permission-blocked');
    expect(state.submitTarget).toBe('permission-response');
    expect(state.sessionActive).toBe(true);
    expect(state.composerLocked).toBe(true);
    expect(state.inputDisabled).toBe(true);
    expect(state.canSubmit).toBe(false);
    expect(state.canStop).toBe(true);
    expect(state.showExternalState).toBe(false);
    expect(state.placeholderKind).toBe('runtime-controlled');
    expect(state.hintKind).toBe('permission-pending');
    expect(state.showStatus).toBe(false);
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
    expect(state.inputDisabled).toBe(false);
    expect(state.canSubmit).toBe(true);
  });

  it('shows local turn submission over a terminal ACP snapshot', () => {
    const state = deriveAcpRuntimeComposerState(baseInput({
      sending: true,
      awaitingResponse: true,
      waitingForOptimisticPrompt: true,
      localTurnInFlight: true,
      turnAccepted: false,
      hasResponseAfterTurn: false,
      acpStatus: 'completed',
      hasTimelineItems: true,
      hasEffectiveEvents: true,
      timelineProcessingKind: 'responding',
    }));

    expect(state.mode).toBe('submitting');
    expect(state.sessionActive).toBe(false);
    expect(state.statusActive).toBe(true);
    expect(state.processingKind).toBe('sending');
    expect(state.inputDisabled).toBe(true);
    expect(state.canSubmit).toBe(false);
    expect(state.canStop).toBe(true);
  });

  it('shows local turn processing after a terminal ACP snapshot accepts the prompt', () => {
    const state = deriveAcpRuntimeComposerState(baseInput({
      awaitingResponse: true,
      localTurnInFlight: true,
      turnAccepted: true,
      hasResponseAfterTurn: false,
      acpStatus: 'completed',
      hasTimelineItems: true,
      hasEffectiveEvents: true,
      timelineProcessingKind: 'responding',
    }));

    expect(state.mode).toBe('normal');
    expect(state.statusActive).toBe(true);
    expect(state.processingKind).toBe('processing');
    expect(state.inputDisabled).toBe(true);
    expect(state.canStop).toBe(true);
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

  it('hides the internal node handoff after a Direct turn completes', () => {
    const state = deriveAcpRuntimeComposerState(baseInput({
      promptQueueEnabled: true,
      lifecycle: lifecycle({
        runtime: { status: 'running', active: true, current: true, phase: 'launching-next-node' },
        acp: { status: 'completed', active: false, stopping: false, terminal: true },
        promptQueue: { revision: 0, items: [], maxItems: 10 },
      }),
      acpStatus: 'completed',
    }));

    expect(state.processingKind).toBe('launching-next-node');
    expect(state.statusActive).toBe(false);
    expect(state.showStatus).toBe(false);
    expect(state.hintKind).toBe('default');
  });

  it('keeps the node handoff visible for Workflow and AUTO runs', () => {
    const state = deriveAcpRuntimeComposerState(baseInput({
      lifecycle: lifecycle({
        runtime: { status: 'running', active: true, current: true, phase: 'launching-next-node' },
        acp: { status: 'completed', active: false, stopping: false, terminal: true },
      }),
      acpStatus: 'completed',
    }));

    expect(state.statusActive).toBe(true);
    expect(state.showStatus).toBe(true);
    expect(state.processingKind).toBe('launching-next-node');
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

describe('mergeConversationAttemptLifecycle', () => {
  it('keeps a newer Direct queue when a stale lifecycle snapshot arrives after stop', () => {
    const local = lifecycle({
      promptQueue: {
        revision: 4,
        maxItems: 10,
        items: [{
          id: 'queued-1',
          content: 'keep visible',
          attachmentCount: 0,
          createdAt: '2026-08-07T00:00:00Z',
        }],
      },
    });
    const incoming = lifecycle({
      runtime: { status: 'paused', active: false, phase: 'paused' },
      promptQueue: { revision: 3, maxItems: 10, items: [] },
    });

    const merged = mergeConversationAttemptLifecycle(local, incoming);
    expect(merged.runtime.status).toBe('paused');
    expect(merged.promptQueue).toEqual(local.promptQueue);
  });

  it('accepts an empty queue when its revision is newer', () => {
    const local = lifecycle({
      promptQueue: {
        revision: 4,
        maxItems: 10,
        items: [{
          id: 'queued-1',
          content: 'deleted',
          attachmentCount: 0,
          createdAt: '2026-08-07T00:00:00Z',
        }],
      },
    });
    const incoming = lifecycle({ promptQueue: { revision: 5, maxItems: 10, items: [] } });

    expect(mergeConversationAttemptLifecycle(local, incoming)).toBe(incoming);
  });
});

describe('shouldKeepLocalRuntimeLifecycleOverride', () => {
  it('keeps continue-started lifecycle over stale paused parent snapshots', () => {
    const localActive = lifecycle({
      runtime: {
        status: 'running',
        outcome: null,
        pauseReason: null,
        resumable: false,
        current: true,
        active: true,
        continuable: false,
        phase: 'provider-running',
      },
      acp: { status: 'cancelled', active: false, stopping: false, terminal: true },
      displayStatus: 'running',
      runtimeDisplay: runningDisplay,
      continueKind: null,
      composer: {
        mode: 'runtime-active',
        submitTarget: 'none',
        processingKind: 'processing',
        statusKey: 'conversation.runtime.runtimeActive',
        canStop: true,
        lockInput: true,
      },
    });
    const stalePaused = lifecycle({
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
      acp: { status: 'cancelled', active: false, stopping: false, terminal: true },
      displayStatus: 'paused',
      runtimeDisplay: pausedDisplay,
      continueKind: 'input',
      composer: {
        mode: 'interrupted-input',
        submitTarget: 'runtime-continue',
        processingKind: 'processing',
        statusKey: null,
        canStop: false,
        lockInput: false,
      },
    });

    expect(shouldKeepLocalRuntimeLifecycleOverride(localActive, stalePaused)).toBe(true);
  });

  it('releases continue-started lifecycle once parent catches up or errors', () => {
    const localActive = lifecycle({
      runtime: {
        status: 'running',
        outcome: null,
        pauseReason: null,
        resumable: false,
        current: true,
        active: true,
        continuable: false,
        phase: 'provider-running',
      },
      composer: {
        mode: 'runtime-active',
        submitTarget: 'none',
        processingKind: 'processing',
        statusKey: 'conversation.runtime.runtimeActive',
        canStop: true,
        lockInput: true,
      },
    });
    const parentActive = lifecycle({
      runtime: {
        status: 'running',
        outcome: null,
        pauseReason: null,
        resumable: false,
        current: true,
        active: true,
        continuable: false,
        phase: 'provider-running',
      },
    });
    const parentError = lifecycle({
      runtime: {
        status: 'paused',
        outcome: null,
        pauseReason: 'runtime-abnormal',
        resumable: true,
        current: true,
        active: false,
        continuable: true,
        phase: 'paused',
      },
      composer: {
        mode: 'runtime-error',
        submitTarget: 'none',
        processingKind: 'processing',
        statusKey: null,
        canStop: false,
        lockInput: true,
      },
    });

    expect(shouldKeepLocalRuntimeLifecycleOverride(localActive, parentActive)).toBe(false);
    expect(shouldKeepLocalRuntimeLifecycleOverride(localActive, parentError)).toBe(false);
  });
});
