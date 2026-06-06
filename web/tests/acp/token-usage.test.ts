import { describe, expect, it } from 'vitest';
import { resolveNodeTokenUsage, formatDisplayToken } from '../../src/lib/token-usage';
import type { NodeDetailVm } from '../../src/types';

function makeNodeDetail(overrides: Partial<NodeDetailVm> = {}): NodeDetailVm {
  return {
    id: 'node-1',
    nodeId: 'node-1',
    sequence: 1,
    label: 'Test Node',
    nodeType: 'agent',
    status: 'completed',
    attemptId: 'attempt-1',
    current: false,
    startedAt: '2026-01-01T00:00:00Z',
    artifactCount: 0,
    attachmentCount: 0,
    artifacts: [],
    attachments: [],
    hasProgressEvents: false,
    hasRawStream: false,
    hasWorkerRef: false,
    manualCheckEnabled: false,
    manualCheckPending: false,
    ...overrides,
  };
}

describe('resolveNodeTokenUsage', () => {
  it('returns null when no conversations and no acpSession', () => {
    const detail = makeNodeDetail();
    expect(resolveNodeTokenUsage(detail)).toBeNull();
  });

  it('returns null when acpSession has no usage', () => {
    const detail = makeNodeDetail({ acpSession: { provider: 'claude', status: 'completed', restored: false } } as any);
    expect(resolveNodeTokenUsage(detail)).toBeNull();
  });

  it('returns null when usage has no breakdown fields', () => {
    const detail = makeNodeDetail({
      acpSession: {
        provider: 'claude',
        status: 'completed',
        restored: false,
        usage: { used: 5000, size: 200000 },
      },
    } as any);
    expect(resolveNodeTokenUsage(detail)).toBeNull();
  });

  // --- Legacy single-session path (no acpConversations) ---

  it('returns usage from acpSession when no conversations (legacy path)', () => {
    const usage = { inputTokens: 1000, outputTokens: 500, totalTokens: 1500 };
    const detail = makeNodeDetail({
      acpSession: { provider: 'claude', status: 'completed', restored: false, usage },
    } as any);
    expect(resolveNodeTokenUsage(detail)).toEqual(usage);
  });

  // --- Single conversation, single attempt ---

  it('accumulates from a single conversation with one attempt', () => {
    const detail = makeNodeDetail({
      acpConversations: [{
        key: 'conv-1',
        label: 'Conv 1',
        sessionMode: 'auto',
        activeAttemptId: 'attempt-1',
        attempts: [{
          nodeId: 'node-1',
          attemptId: 'attempt-1',
          status: 'completed',
          current: false,
          acpSession: {
            provider: 'claude',
            status: 'completed',
            restored: false,
            usage: { used: 12000, size: 200000, inputTokens: 8000, outputTokens: 4000, totalTokens: 12000 },
          },
        }],
      }],
    } as any);
    const result = resolveNodeTokenUsage(detail)!;
    expect(result.inputTokens).toBe(8000);
    expect(result.outputTokens).toBe(4000);
    expect(result.totalTokens).toBe(12000);
    expect(result.used).toBe(12000);
    expect(result.size).toBe(200000);
  });

  // --- Multiple attempts ---

  it('accumulates token counts across multiple attempts', () => {
    const detail = makeNodeDetail({
      acpConversations: [{
        key: 'conv-1',
        label: 'Conv 1',
        sessionMode: 'auto',
        activeAttemptId: 'attempt-2',
        attempts: [
          {
            nodeId: 'node-1',
            attemptId: 'attempt-1',
            status: 'error',
            current: false,
            acpSession: {
              provider: 'claude',
              status: 'completed',
              restored: false,
              usage: { inputTokens: 5000, outputTokens: 2000, totalTokens: 7000, used: 7000, size: 200000 },
            },
          },
          {
            nodeId: 'node-1',
            attemptId: 'attempt-2',
            status: 'completed',
            current: true,
            acpSession: {
              provider: 'claude',
              status: 'completed',
              restored: false,
              usage: { inputTokens: 8000, outputTokens: 3000, totalTokens: 11000, used: 11000, size: 200000 },
            },
          },
        ],
      }],
    } as any);
    const result = resolveNodeTokenUsage(detail)!;
    expect(result.inputTokens).toBe(13000);   // 5000 + 8000
    expect(result.outputTokens).toBe(5000);    // 2000 + 3000
    expect(result.totalTokens).toBe(18000);    // 7000 + 11000
    expect(result.used).toBe(11000);            // latest attempt
    expect(result.size).toBe(200000);           // latest attempt
  });

  // --- Multiple conversations ---

  it('accumulates across multiple conversations', () => {
    const detail = makeNodeDetail({
      acpConversations: [
        {
          key: 'conv-1',
          label: 'Conv 1',
          sessionMode: 'auto',
          activeAttemptId: 'a1',
          attempts: [{
            nodeId: 'node-1',
            attemptId: 'a1',
            status: 'completed',
            current: false,
            acpSession: {
              provider: 'claude',
              status: 'completed',
              restored: false,
              usage: { inputTokens: 5000, outputTokens: 1000, totalTokens: 6000, used: 6000, size: 200000 },
            },
          }],
        },
        {
          key: 'conv-2',
          label: 'Conv 2',
          sessionMode: 'auto',
          activeAttemptId: 'a2',
          attempts: [{
            nodeId: 'node-1',
            attemptId: 'a2',
            status: 'completed',
            current: true,
            acpSession: {
              provider: 'claude',
              status: 'completed',
              restored: false,
              usage: { inputTokens: 3000, outputTokens: 800, totalTokens: 3800, used: 3800, size: 200000 },
            },
          }],
        },
      ],
    } as any);
    const result = resolveNodeTokenUsage(detail)!;
    expect(result.inputTokens).toBe(8000);   // 5000 + 3000
    expect(result.outputTokens).toBe(1800);   // 1000 + 800
    expect(result.totalTokens).toBe(9800);    // 6000 + 3800
    expect(result.used).toBe(3800);            // latest attempt of last conversation
    expect(result.size).toBe(200000);
  });

  // --- Edge cases ---

  it('handles attempts with no acpSession', () => {
    const detail = makeNodeDetail({
      acpConversations: [{
        key: 'conv-1',
        label: 'Conv 1',
        sessionMode: 'auto',
        activeAttemptId: 'a1',
        attempts: [
          {
            nodeId: 'node-1',
            attemptId: 'a1',
            status: 'error',
            current: false,
            // no acpSession
          },
          {
            nodeId: 'node-1',
            attemptId: 'a2',
            status: 'completed',
            current: true,
            acpSession: {
              provider: 'claude',
              status: 'completed',
              restored: false,
              usage: { inputTokens: 5000, outputTokens: 2000, totalTokens: 7000 },
            },
          },
        ],
      }],
    } as any);
    const result = resolveNodeTokenUsage(detail)!;
    expect(result.inputTokens).toBe(5000);
    expect(result.outputTokens).toBe(2000);
    expect(result.totalTokens).toBe(7000);
  });

  it('handles attempts with usage that has no breakdown fields', () => {
    const detail = makeNodeDetail({
      acpConversations: [{
        key: 'conv-1',
        label: 'Conv 1',
        sessionMode: 'auto',
        activeAttemptId: 'a1',
        attempts: [{
          nodeId: 'node-1',
          attemptId: 'a1',
          status: 'completed',
          current: false,
          acpSession: {
            provider: 'claude',
            status: 'completed',
            restored: false,
            usage: { used: 5000, size: 200000 },  // no breakdown fields
          },
        }],
      }],
    } as any);
    expect(resolveNodeTokenUsage(detail)).toBeNull();
  });

  it('skips null/undefined token fields in accumulation', () => {
    const detail = makeNodeDetail({
      acpConversations: [{
        key: 'conv-1',
        label: 'Conv 1',
        sessionMode: 'auto',
        activeAttemptId: 'a1',
        attempts: [{
          nodeId: 'node-1',
          attemptId: 'a1',
          status: 'completed',
          current: false,
          acpSession: {
            provider: 'claude',
            status: 'completed',
            restored: false,
            usage: { inputTokens: 5000, totalTokens: 5000 },  // outputTokens omitted
          },
        }],
      }],
    } as any);
    const result = resolveNodeTokenUsage(detail)!;
    expect(result.inputTokens).toBe(5000);
    expect(result.outputTokens).toBeUndefined();
    expect(result.totalTokens).toBe(5000);
  });

  it('takes used/size from the last attempt with data', () => {
    const detail = makeNodeDetail({
      acpConversations: [{
        key: 'conv-1',
        label: 'Conv 1',
        sessionMode: 'auto',
        activeAttemptId: 'a2',
        attempts: [
          {
            nodeId: 'node-1',
            attemptId: 'a1',
            status: 'completed',
            current: false,
            acpSession: {
              provider: 'claude',
              status: 'completed',
              restored: false,
              usage: { inputTokens: 5000, totalTokens: 5000, used: 5000, size: 200000 },
            },
          },
          {
            nodeId: 'node-1',
            attemptId: 'a2',
            status: 'completed',
            current: true,
            acpSession: {
              provider: 'claude',
              status: 'completed',
              restored: false,
              usage: { inputTokens: 3000, totalTokens: 3000, used: 3000, size: 100000 },
            },
          },
        ],
      }],
    } as any);
    const result = resolveNodeTokenUsage(detail)!;
    expect(result.used).toBe(3000);     // latest, not accumulated
    expect(result.size).toBe(100000);    // latest
    expect(result.inputTokens).toBe(8000); // accumulated: 5000 + 3000
    expect(result.totalTokens).toBe(8000); // accumulated: 5000 + 3000
  });
});

describe('formatDisplayToken', () => {
  it('returns "-" for null', () => {
    expect(formatDisplayToken(null)).toBe('-');
  });

  it('returns "-" for undefined', () => {
    expect(formatDisplayToken(undefined)).toBe('-');
  });

  it('delegates to formatTokenCount for numbers', () => {
    expect(formatDisplayToken(0)).toBe('0');
    expect(formatDisplayToken(842)).toBe('842');
    expect(formatDisplayToken(12000)).toBe('12.0K');
    expect(formatDisplayToken(1_500_000)).toBe('1.5M');
  });
});
