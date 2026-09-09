import { describe, expect, it } from 'vitest';
import type { eventWithTime } from '@rrweb/types';
import { canReplay, createRecordingBuffer } from '../rrweb-demo/recording';

const event = (type: number, timestamp = 1000) => ({ type, timestamp, data: {} }) as eventWithTime;

describe('rrweb demo recording boundary', () => {
  it('requires a full snapshot before playback', () => {
    const buffer = createRecordingBuffer();
    buffer.append(event(4));
    buffer.append(event(3));
    expect(canReplay(buffer.stop())).toBe(false);
    const complete = createRecordingBuffer();
    complete.append(event(4));
    complete.append(event(2));
    expect(canReplay(complete.stop())).toBe(true);
  });
  it('keeps the initial snapshot when reaching the event cap', () => {
    const buffer = createRecordingBuffer({ events: 2, bytes: 10000 });
    buffer.append(event(4));
    buffer.append(event(2));
    expect(buffer.append(event(3))).toBe('events');
    expect(buffer.stop('events').events.map((item) => item.type)).toEqual([4, 2]);
  });
  it('counts UTF-8 bytes and refuses the event that exceeds the limit', () => {
    const data = { ...event(5), data: { tag: '中文', payload: {} } } as eventWithTime;
    const probe = createRecordingBuffer();
    probe.append(data);
    const exact = probe.stop('duration').bytes;
    const buffer = createRecordingBuffer({ bytes: exact, events: 5 });
    expect(buffer.append(data)).toBeUndefined();
    expect(buffer.append(data)).toBe('bytes');
    const result = buffer.stop('bytes');
    expect(result.bytes).toBe(new TextEncoder().encode(JSON.stringify(result)).byteLength);
    expect(result.bytes).toBeLessThanOrEqual(exact);
    expect(buffer.count).toBe(1);
  });
  it('stops idempotently and rejects late events', () => {
    const buffer = createRecordingBuffer();
    buffer.append(event(2));
    const result = buffer.stop('duration');
    buffer.append(event(3));
    expect(buffer.stop()).toBe(result);
    expect(result.reason).toBe('duration');
    expect(result.events).toHaveLength(1);
  });
  it('retains the first budget stop even when the caller stops manually', () => {
    const buffer = createRecordingBuffer({ events: 1, bytes: 10000 });
    buffer.append(event(2));
    expect(buffer.append(event(3))).toBe('events');
    expect(buffer.stop().reason).toBe('events');
  });
  it('reports the actual serialized recording bytes, including the envelope', () => {
    const buffer = createRecordingBuffer();
    buffer.append({ ...event(5), data: { tag: '中文', payload: {} } } as eventWithTime);
    const result = buffer.stop();
    expect(result.bytes).toBe(new TextEncoder().encode(JSON.stringify(result)).byteLength);
  });
});
