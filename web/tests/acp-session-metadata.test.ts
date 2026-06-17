import { describe, expect, it } from 'vitest';
import { hasAcpSessionMetadata } from '@/lib/acp-session-shell';

describe('hasAcpSessionMetadata', () => {
  const configWithOptions = {
    currentModelId: null,
    currentModeId: null,
    configOptions: [
      {
        category: 'model',
        options: [{ value: 'opus', name: 'Opus' }],
      },
      {
        category: 'mode',
        options: [{ value: 'default', name: 'Default' }],
      },
    ],
  };

  it('reports missing when session is null or undefined', () => {
    expect(hasAcpSessionMetadata(null)).toBe(false);
    expect(hasAcpSessionMetadata(undefined)).toBe(false);
  });

  it('reports missing when system prompt is absent', () => {
    expect(hasAcpSessionMetadata({
      systemPromptAppend: null,
      config: configWithOptions,
    })).toBe(false);
  });

  it('reports available when system prompt and config options exist without current values', () => {
    expect(hasAcpSessionMetadata({
      systemPromptAppend: 'You are a helpful assistant.',
      config: configWithOptions,
    })).toBe(true);
  });

  it('reports available when grouped model and mode options exist', () => {
    expect(hasAcpSessionMetadata({
      systemPromptAppend: 'You are a helpful assistant.',
      config: {
        models: { availableModels: [{ modelId: 'opus', name: 'Opus' }] },
        modes: { availableModes: [{ id: 'default', name: 'Default' }] },
      },
    })).toBe(true);
  });

  it('reports available when current ids exist for legacy snapshots', () => {
    expect(hasAcpSessionMetadata({
      systemPromptAppend: 'You are a helpful assistant.',
      config: {
        currentModelId: 'claude-opus-4-8',
        currentModeId: 'acceptEdits',
      },
    })).toBe(true);
  });

  it('reports missing when system prompt is empty string', () => {
    expect(hasAcpSessionMetadata({
      systemPromptAppend: '',
      config: configWithOptions,
    })).toBe(false);
  });

  it('reports missing when either model or permission choices are absent', () => {
    expect(hasAcpSessionMetadata({
      systemPromptAppend: 'You are a helpful assistant.',
      config: {
        configOptions: [{ category: 'model', options: [{ value: 'opus', name: 'Opus' }] }],
      },
    })).toBe(false);
  });

  it('reports missing when config choices are absent', () => {
    expect(hasAcpSessionMetadata({
      systemPromptAppend: 'You are a helpful assistant.',
      config: {},
    })).toBe(false);
  });
});
