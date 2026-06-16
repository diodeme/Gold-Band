import { describe, expect, it } from 'vitest';
import { hasAcpSessionMetadata } from '@/lib/acp-session-shell';

describe('hasAcpSessionMetadata', () => {
  it('reports missing when session is null or undefined', () => {
    expect(hasAcpSessionMetadata(null)).toBe(false);
    expect(hasAcpSessionMetadata(undefined)).toBe(false);
  });

  it('reports missing when system prompt is absent', () => {
    expect(hasAcpSessionMetadata({
      systemPromptAppend: null,
      currentModelId: 'claude-opus-4-8',
      currentModeId: 'acceptEdits',
    })).toBe(false);
  });

  it('reports missing when currentModelId is absent', () => {
    expect(hasAcpSessionMetadata({
      systemPromptAppend: 'You are a helpful assistant.',
      currentModelId: null,
      currentModeId: 'acceptEdits',
    })).toBe(false);
  });

  it('reports missing when currentModeId is absent', () => {
    expect(hasAcpSessionMetadata({
      systemPromptAppend: 'You are a helpful assistant.',
      currentModelId: 'claude-opus-4-8',
      currentModeId: null,
    })).toBe(false);
  });

  it('reports available when all three metadata fields exist', () => {
    expect(hasAcpSessionMetadata({
      systemPromptAppend: 'You are a helpful assistant.',
      currentModelId: 'claude-opus-4-8',
      currentModeId: 'acceptEdits',
    })).toBe(true);
  });

  it('reports missing when system prompt is empty string', () => {
    expect(hasAcpSessionMetadata({
      systemPromptAppend: '',
      currentModelId: 'claude-opus-4-8',
      currentModeId: 'acceptEdits',
    })).toBe(false);
  });
});
