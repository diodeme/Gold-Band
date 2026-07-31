import { describe, expect, it } from 'vitest';
import {
  limitAcpEvents,
  loadedEventBufferLimit,
  mergeAcpEvents,
} from '../../src/components/acp/ACPChatDialog';
import type { AcpUiEventVm } from '../../src/types';

function event(
  partial: Partial<AcpUiEventVm> &
    Pick<AcpUiEventVm, 'id' | 'seq' | 'timestamp' | 'kind'>,
): AcpUiEventVm {
  return {
    id: partial.id,
    seq: partial.seq,
    timestamp: partial.timestamp,
    kind: partial.kind,
    ...partial,
  } as AcpUiEventVm;
}

describe('ACPChatDialog pagination buffer', () => {
  it('keeps three configured pages in the sliding event buffer', () => {
    expect(loadedEventBufferLimit(360)).toBe(1080);
    expect(loadedEventBufferLimit(30)).toBe(90);
    expect(loadedEventBufferLimit(10)).toBe(30);
    expect(loadedEventBufferLimit(2000)).toBe(2000);
  });

  it('keeps the current page when the next page is merged', () => {
    const current = Array.from({ length: 360 }, (_, index) =>
      event({
        id: `current-${index + 1}`,
        seq: index + 1,
        timestamp: `${index + 1}Z`,
        kind: 'textDelta',
        content: `current ${index + 1}`,
      }),
    );
    const newer = Array.from({ length: 360 }, (_, index) =>
      event({
        id: `newer-${index + 361}`,
        seq: index + 361,
        timestamp: `${index + 361}Z`,
        kind: 'textDelta',
        content: `newer ${index + 361}`,
      }),
    );

    const merged = limitAcpEvents(
      mergeAcpEvents(current, newer),
      'start',
      loadedEventBufferLimit(360),
    );

    expect(merged).toHaveLength(720);
    expect(merged[0]!.id).toBe('current-1');
    expect(merged[359]!.id).toBe('current-360');
    expect(merged[360]!.id).toBe('newer-361');
    expect(merged[719]!.id).toBe('newer-720');
  });

  it('slides a full three-page window without breaking the page boundary', () => {
    const current = Array.from({ length: 1080 }, (_, index) =>
      event({
        id: `event-${index + 1}`,
        seq: index + 1,
        timestamp: `${index + 1}Z`,
        kind: 'textDelta',
        content: `event ${index + 1}`,
      }),
    );
    const newer = Array.from({ length: 360 }, (_, index) =>
      event({
        id: `event-${index + 1081}`,
        seq: index + 1081,
        timestamp: `${index + 1081}Z`,
        kind: 'textDelta',
        content: `event ${index + 1081}`,
      }),
    );

    const merged = limitAcpEvents(
      mergeAcpEvents(current, newer),
      'start',
      loadedEventBufferLimit(360),
    );

    expect(merged).toHaveLength(1080);
    expect(merged[0]!.seq).toBe(361);
    expect(merged[719]!.seq).toBe(1080);
    expect(merged[720]!.seq).toBe(1081);
    expect(merged[1079]!.seq).toBe(1440);
  });
});
