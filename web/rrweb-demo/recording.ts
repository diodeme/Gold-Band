import type { eventWithTime } from '@rrweb/types';

export const RECORDING_LIMITS = { durationMs: 60_000, bytes: 12 * 1024 * 1024, events: 12_000 } as const;
export const CAPTURE_SIZE = { width: 1440, height: 800, medium: 900, narrow: 600 } as const;
export type StopReason = 'manual' | 'duration' | 'bytes' | 'events';
export type Recording = { version: 1; events: eventWithTime[]; bytes: number; reason: StopReason };

export function createRecordingBuffer(limits: { bytes: number; events: number } = RECORDING_LIMITS) {
  const events: eventWithTime[] = [];
  const encoder = new TextEncoder();
  let bytes = 2;
  let stopped: Recording | undefined;
  return {
    append(event: eventWithTime): StopReason | undefined {
      if (stopped) return stopped.reason;
      if (events.length >= limits.events) return 'events';
      const size = encoder.encode(JSON.stringify(event)).byteLength + (events.length ? 1 : 0);
      if (bytes + size > limits.bytes) return 'bytes';
      events.push(event);
      bytes += size;
      return undefined;
    },
    stop(reason: StopReason = 'manual'): Recording {
      stopped ??= { version: 1, events, bytes, reason };
      return stopped;
    },
    get count() { return events.length; },
  };
}

export function canReplay(recording: Recording) {
  return recording.events.length >= 2 && recording.events.some((event) => event.type === 2);
}

export interface CaptureApi {
  start: () => void;
  stop: (reason?: StopReason) => Recording;
  status: () => { recording: boolean; count: number };
  mark: (name: string) => void;
}

declare global {
  interface Window { goldBandCapture?: CaptureApi }
}
