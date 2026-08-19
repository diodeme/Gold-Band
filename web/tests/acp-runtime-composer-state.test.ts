import { describe, expect, it } from 'vitest';
import {
  deriveAcpRuntimeComposerState,
  isAcceptedQueuePromptSubmitKind,
  mergeConversationAttemptLifecycle,
  shouldKeepLocalRuntimeLifecycleOverride,
  shouldSettleRuntimeContinueSubmission,
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
    control: { mode: 'non-runtime-controlled' },
    acp: {
      sessionAvailability: 'established',
      liveTurnActivity: 'idle',
      latestTurnStatus: 'completed',
      stopping: false,
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
    } else if (merged.runtime.active || merged.acp.liveTurnActivity !== 'idle') {
      merged.composer = {
        ...merged.composer,
        mode: 'runtime-active',
        submitTarget: 'none',
        processingKind: merged.runtime.phase === 'launching-next-node' ? 'launching-next-node' : 'processing',
        canStop: true,
        lockInput: true,
      };
    } else if (merged.continueKind === 'action') {
      merged.composer = { ...merged.composer, mode: 'normal', submitTarget: 'acp-prompt', lockInput: false };
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
  it('allows an attachment-only prompt while keeping a completely empty prompt disabled', () => {
    const attachmentOnly = deriveAcpRuntimeComposerState(baseInput({
      prompt: '',
      hasAttachments: true,
    }));
    const empty = deriveAcpRuntimeComposerState(baseInput({
      prompt: '',
      hasAttachments: false,
    }));

    expect(attachmentOnly.canSubmit).toBe(true);
    expect(empty.canSubmit).toBe(false);
  });

  it('keeps a new-session composer locked while its first timeline item catches up', () => {
    const state = deriveAcpRuntimeComposerState(baseInput({
      promptQueueEnabled: true,
      lifecycle: lifecycle(),
      acpStatus: 'completed',
      hasTimelineItems: false,
      hasEffectiveEvents: false,
      initialTimelinePending: true,
    }));

    expect(state.inputDisabled).toBe(true);
    expect(state.canSubmit).toBe(false);
    expect(state.showStatus).toBe(true);
    expect(state.processingKind).toBe('launching');
    expect(state.placeholderKind).toBe('runtime-controlled');
  });

  it('restores a normal composer after an empty current Direct attempt settles as cancelled', () => {
    const state = deriveAcpRuntimeComposerState(baseInput({
      promptQueueEnabled: true,
      lifecycle: lifecycle({
        runtime: {
          status: 'paused',
          active: false,
          current: true,
          phase: 'paused',
          pauseReason: 'process-interrupted',
        },
        acp: {
          sessionAvailability: 'established',
          liveTurnActivity: 'idle',
          latestTurnStatus: 'cancelled',
          stopping: false,
        },
      }),
      acpStatus: 'cancelled',
      hasTimelineItems: false,
      hasEffectiveEvents: false,
      initialTimelinePending: false,
    }));

    expect(state.mode).toBe('normal');
    expect(state.inputDisabled).toBe(false);
    expect(state.canSubmit).toBe(true);
    expect(state.showStatus).toBe(false);
    expect(state.processingKind).toBe('processing');
  });

  it('keeps the Direct composer editable and queues submissions while a turn is active', () => {
    const state = deriveAcpRuntimeComposerState(baseInput({
      lifecycle: lifecycle({
        runtime: { status: 'running', active: true, current: true, phase: 'provider-running' },
        acp: { sessionAvailability: 'established', liveTurnActivity: 'running', latestTurnStatus: 'none', stopping: false },
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
        continueKind: 'action',
        composer: { mode: 'normal', submitTarget: 'acp-prompt', lockInput: false },
      }),
      sending: true,
      prompt: 'next prompt',
    }));
    const runningSending = deriveAcpRuntimeComposerState(baseInput({
      lifecycle: lifecycle({
        runtime: { status: 'running', active: true, current: true, phase: 'provider-running' },
        acp: { sessionAvailability: 'established', liveTurnActivity: 'running', latestTurnStatus: 'none', stopping: false },
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
      quoteCount: 0,
      createdAt: '2026-08-07T00:00:00Z',
    }));
    const state = deriveAcpRuntimeComposerState(baseInput({
      lifecycle: lifecycle({
        runtime: { status: 'running', active: true, current: true, phase: 'provider-running' },
        acp: { sessionAvailability: 'established', liveTurnActivity: 'running', latestTurnStatus: 'none', stopping: false },
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
        acp: { sessionAvailability: 'established', liveTurnActivity: 'running', latestTurnStatus: 'none', stopping: false },
      }),
      acpStatus: 'running',
    }));

    expect(state.submitTarget).toBe('none');
    expect(state.inputDisabled).toBe(true);
    expect(state.canSubmit).toBe(false);
  });

  it('keeps a superseded session locked even when stale ACP activity still appears active', () => {
    const state = deriveAcpRuntimeComposerState(baseInput({
      lifecycle: lifecycle({
        acp: {
          sessionAvailability: 'established',
          liveTurnActivity: 'running',
          latestTurnStatus: 'none',
          stopping: false,
        },
        composer: {
          mode: 'session-superseded',
          submitTarget: 'none',
          lockInput: true,
          canStop: false,
          supersededBy: {
            roundId: 'round-001',
            nodeId: 'review',
            attemptId: 'attempt-003',
            pathLabel: 'review/attempt-003',
          },
        },
      }),
      acpStatus: 'running',
      localTurnInFlight: true,
    }));

    expect(state.mode).toBe('session-superseded');
    expect(state.submitTarget).toBe('none');
    expect(state.inputDisabled).toBe(true);
    expect(state.canSubmit).toBe(false);
    expect(state.canStop).toBe(false);
    expect(state.showStatus).toBe(false);
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
        acp: { sessionAvailability: 'established', liveTurnActivity: 'running', latestTurnStatus: 'none', stopping: false },
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
          sessionAvailability: 'established',
          liveTurnActivity: 'running',
          latestTurnStatus: 'none',
          stopping: false,
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
        acp: { sessionAvailability: 'closing', liveTurnActivity: 'cancel-requested', latestTurnStatus: 'none', stopping: true },
        displayStatus: 'cancelling',
        runtimeDisplay: pausedDisplay,
        continueKind: 'action',
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
        acp: { sessionAvailability: 'established', liveTurnActivity: 'idle', latestTurnStatus: 'cancelled', stopping: false },
        displayStatus: 'cancelling',
        runtimeDisplay: pausedDisplay,
        continueKind: 'action',
      }),
      acpStatus: 'cancelled',
      cancelling: true,
      stopCommandPending: true,
    }));

    expect(state.stopInProgress).toBe(false);
    expect(state.mode).toBe('normal');
    expect(state.inputDisabled).toBe(false);
    expect(state.canStop).toBe(false);
  });

  it('keeps process-interrupted stopped input as a non-runtime ACP prompt', () => {
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
        acp: { sessionAvailability: 'established', liveTurnActivity: 'idle', latestTurnStatus: 'cancelled', stopping: false },
        displayStatus: 'paused',
        runtimeDisplay: pausedDisplay,
        continueKind: 'action',
      }),
      acpStatus: 'cancelled',
    }));

    expect(state.mode).toBe('normal');
    expect(state.submitTarget).toBe('acp-prompt');
    expect(state.inputDisabled).toBe(false);
    expect(state.canSubmit).toBe(true);
  });

  it('keeps repair-exhausted runtime-abnormal input open for user-guided continue', () => {
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
        acp: { sessionAvailability: 'established', liveTurnActivity: 'idle', latestTurnStatus: 'cancelled', stopping: false },
        displayStatus: 'runtime-abnormal',
        runtimeDisplay: runtimeAbnormalDisplay,
        continueKind: 'action',
      }),
      acpStatus: 'cancelled',
    }));

    expect(state.mode).toBe('normal');
    expect(state.submitTarget).toBe('acp-prompt');
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
        acp: { sessionAvailability: 'established', liveTurnActivity: 'idle', latestTurnStatus: 'failed', stopping: false },
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
        acp: { sessionAvailability: 'established', liveTurnActivity: 'idle', latestTurnStatus: 'failed', stopping: false },
        displayStatus: 'runtime-abnormal',
        runtimeDisplay: runtimeAbnormalDisplay,
        continueKind: 'action',
      }),
      acpStatus: 'failed',
      runtimeErrorMessage: '当前会话运行失败，请查看错误原因',
    }));

    expect(abnormalState.mode).toBe('normal');
    expect(abnormalState.submitTarget).toBe('acp-prompt');
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
        acp: { sessionAvailability: 'established', liveTurnActivity: 'idle', latestTurnStatus: 'cancelled', stopping: false },
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
          acp: { sessionAvailability: 'established', liveTurnActivity: 'idle', latestTurnStatus: acpStatus, stopping: false },
          displayStatus: 'paused',
          runtimeDisplay: pausedDisplay,
          continueKind: 'action',
        }),
        acpStatus,
        cancelling: true,
        stopCommandPending: true,
        awaitingResponse: true,
        turnAccepted: true,
        hasResponseAfterTurn: false,
      }));

      expect(state.mode).toBe('normal');
      expect(state.stopInProgress).toBe(false);
      expect(state.sessionActive).toBe(false);
      expect(state.statusActive).toBe(false);
      expect(state.processingKind).toBe('responding');
      expect(state.submitTarget).toBe('acp-prompt');
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
        acp: { sessionAvailability: 'established', liveTurnActivity: 'idle', latestTurnStatus: 'cancelled', stopping: false },
        displayStatus: 'paused',
        runtimeDisplay: pausedDisplay,
        continueKind: 'action',
      }),
      acpStatus: 'cancelled',
      waitingForOptimisticPrompt: true,
      awaitingResponse: true,
      turnAccepted: false,
      hasResponseAfterTurn: false,
    }));

    expect(state.mode).toBe('normal');
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
        acp: { sessionAvailability: 'established', liveTurnActivity: 'running', latestTurnStatus: 'none', stopping: false },
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

  it('keeps permission waits locked when the session has no prompt queue', () => {
    const state = deriveAcpRuntimeComposerState(baseInput({
      lifecycle: lifecycle(),
      waitingForPermission: true,
      prompt: 'allow?',
    }));

    expect(state.mode).toBe('permission-blocked');
    expect(state.submitTarget).toBe('none');
    expect(state.sessionActive).toBe(true);
    expect(state.composerLocked).toBe(true);
    expect(state.inputDisabled).toBe(true);
    expect(state.canSubmit).toBe(false);
    expect(state.canStop).toBe(true);
    expect(state.showExternalState).toBe(false);
    expect(state.placeholderKind).toBe('runtime-controlled');
    expect(state.showStatus).toBe(false);
  });

  it('routes a Direct message to the existing queue while permission remains pending', () => {
    const state = deriveAcpRuntimeComposerState(baseInput({
      lifecycle: lifecycle({
        acp: {
          sessionAvailability: 'established',
          liveTurnActivity: 'running',
          latestTurnStatus: 'none',
          stopping: false,
        },
        promptQueue: { revision: 0, items: [], maxItems: 10 },
      }),
      promptQueueEnabled: true,
      waitingForPermission: true,
      prompt: '排队发送',
    }));

    expect(state.mode).toBe('permission-blocked');
    expect(state.submitTarget).toBe('queue-prompt');
    expect(state.composerLocked).toBe(false);
    expect(state.inputDisabled).toBe(false);
    expect(state.canSubmit).toBe(true);
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
        acp: { sessionAvailability: 'established', liveTurnActivity: 'idle', latestTurnStatus: 'completed', stopping: false },
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

  it('shows workspace preparation from the backend lifecycle', () => {
    const state = deriveAcpRuntimeComposerState(baseInput({
      lifecycle: lifecycle({
        runtime: { status: 'completed', active: false, current: true, phase: 'preparing-workspace' },
        acp: { sessionAvailability: 'established', liveTurnActivity: 'idle', latestTurnStatus: 'completed', stopping: false },
        composer: {
          mode: 'runtime-active',
          submitTarget: 'none',
          processingKind: 'preparing-workspace',
          canStop: true,
          lockInput: true,
        },
      }),
      acpStatus: 'completed',
      awaitingResponse: true,
      turnAccepted: true,
      hasResponseAfterTurn: false,
    }));

    expect(state.mode).toBe('runtime-active');
    expect(state.processingKind).toBe('preparing-workspace');
    expect(state.canStop).toBe(true);
  });

  it('keeps stopping ahead of workspace preparation after stop is clicked', () => {
    const state = deriveAcpRuntimeComposerState(baseInput({
      lifecycle: lifecycle({
        runtime: { status: 'completed', active: false, current: true, phase: 'preparing-workspace' },
        acp: { sessionAvailability: 'established', liveTurnActivity: 'idle', latestTurnStatus: 'completed', stopping: false },
        composer: {
          mode: 'runtime-active',
          submitTarget: 'none',
          processingKind: 'preparing-workspace',
          canStop: true,
          lockInput: true,
        },
      }),
      acpStatus: 'completed',
      stopCommandPending: true,
    }));

    expect(state.mode).toBe('stopping');
    expect(state.processingKind).toBe('stopping');
    expect(state.stopInProgress).toBe(true);
  });

  it('hides the internal node handoff after a Direct turn completes', () => {
    const state = deriveAcpRuntimeComposerState(baseInput({
      promptQueueEnabled: true,
      lifecycle: lifecycle({
        runtime: { status: 'running', active: true, current: true, phase: 'launching-next-node' },
        acp: { sessionAvailability: 'established', liveTurnActivity: 'idle', latestTurnStatus: 'completed', stopping: false },
        promptQueue: { revision: 0, items: [], maxItems: 10 },
      }),
      acpStatus: 'completed',
    }));

    expect(state.processingKind).toBe('launching-next-node');
    expect(state.statusActive).toBe(false);
    expect(state.showStatus).toBe(false);
  });

  it('keeps the node handoff visible for Workflow and AUTO runs', () => {
    const state = deriveAcpRuntimeComposerState(baseInput({
      lifecycle: lifecycle({
        runtime: { status: 'running', active: true, current: true, phase: 'launching-next-node' },
        acp: { sessionAvailability: 'established', liveTurnActivity: 'idle', latestTurnStatus: 'completed', stopping: false },
      }),
      acpStatus: 'completed',
    }));

    expect(state.statusActive).toBe(true);
    expect(state.showStatus).toBe(true);
    expect(state.processingKind).toBe('launching-next-node');
  });

  it('does not block non-runtime conversation when workflow validation fails', () => {
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
        continueKind: 'action',
      }),
    }));

    expect(completed.mode).toBe('normal');
    expect(completed.submitTarget).toBe('acp-prompt');
    expect(interrupted.mode).toBe('normal');
    expect(interrupted.submitTarget).toBe('acp-prompt');
  });
});

describe('mergeConversationAttemptLifecycle', () => {
  it('rejects a late stopping facet after the same turn already became terminal', () => {
    const terminal = lifecycle({
      acp: {
        revision: 42,
        turnId: 'turn-1',
        liveTurnActivity: 'idle',
        latestTurnStatus: 'cancelled',
        stopping: false,
      },
    });
    const lateAccepted = lifecycle({
      acp: {
        revision: 41,
        turnId: 'turn-1',
        sessionAvailability: 'closing',
        liveTurnActivity: 'cancel-requested',
        latestTurnStatus: 'none',
        stopping: true,
      },
    });

    const merged = mergeConversationAttemptLifecycle(terminal, lateAccepted);

    expect(merged.acp).toBe(terminal.acp);
    expect(merged.acp.latestTurnStatus).toBe('cancelled');
    expect(merged.acp.stopping).toBe(false);
    expect(merged.composer.mode).toBe('normal');
  });

  it('keeps terminal dominance when duplicate revisions arrive out of order', () => {
    const terminal = lifecycle({
      acp: {
        revision: 42,
        turnId: 'turn-1',
        liveTurnActivity: 'idle',
        latestTurnStatus: 'cancelled',
        stopping: false,
      },
    });
    const staleRunning = lifecycle({
      acp: {
        revision: 42,
        turnId: 'turn-1',
        liveTurnActivity: 'running',
        latestTurnStatus: 'none',
        stopping: false,
      },
    });

    expect(mergeConversationAttemptLifecycle(terminal, staleRunning).acp).toBe(terminal.acp);
  });

  it('keeps a newer Direct queue when a stale lifecycle snapshot arrives after stop', () => {
    const local = lifecycle({
      promptQueue: {
        revision: 4,
        maxItems: 10,
        items: [{
          id: 'queued-1',
          content: 'keep visible',
          attachmentCount: 0,
          quoteCount: 0,
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
          quoteCount: 0,
          createdAt: '2026-08-07T00:00:00Z',
        }],
      },
    });
    const incoming = lifecycle({ promptQueue: { revision: 5, maxItems: 10, items: [] } });

    expect(mergeConversationAttemptLifecycle(local, incoming)).toBe(incoming);
  });
});

describe('shouldSettleRuntimeContinueSubmission', () => {
  it('keeps the pending label until the lifecycle removes the continue action', () => {
    expect(shouldSettleRuntimeContinueSubmission(true, true)).toBe(false);
    expect(shouldSettleRuntimeContinueSubmission(true, false)).toBe(true);
    expect(shouldSettleRuntimeContinueSubmission(false, false)).toBe(false);
  });
});

describe('isAcceptedQueuePromptSubmitKind', () => {
  it('accepts both a durable enqueue and an idle-boundary direct ACP send', () => {
    expect(isAcceptedQueuePromptSubmitKind('queued')).toBe(true);
    expect(isAcceptedQueuePromptSubmitKind('acp-session')).toBe(true);
  });

  it('keeps unrelated or rejected command outcomes on the failure path', () => {
    expect(isAcceptedQueuePromptSubmitKind('rejected')).toBe(false);
    expect(isAcceptedQueuePromptSubmitKind('runtime-continue-started')).toBe(false);
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
      acp: { sessionAvailability: 'established', liveTurnActivity: 'idle', latestTurnStatus: 'cancelled', stopping: false },
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
      acp: { sessionAvailability: 'established', liveTurnActivity: 'idle', latestTurnStatus: 'cancelled', stopping: false },
      displayStatus: 'paused',
      runtimeDisplay: pausedDisplay,
      continueKind: 'action',
      composer: {
        mode: 'normal',
        submitTarget: 'acp-prompt',
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
