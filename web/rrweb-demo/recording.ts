import type { eventWithTime } from '@rrweb/types';

export const RECORDING_LIMITS = { durationMs: 60_000, bytes: 12 * 1024 * 1024, events: 12_000 } as const;
export const CAPTURE_SIZE = { width: 1440, height: 800, medium: 900, narrow: 600 } as const;
export type StopReason = 'manual' | 'duration' | 'bytes' | 'events';
export type Recording = { version: 1; events: eventWithTime[]; bytes: number; reason: StopReason };

// Include the self-describing byte count without serializing all previous events per append.
function recordingBytes(arrayBytes: number, reason: StopReason) {
  const envelope = new TextEncoder().encode(JSON.stringify({ version: 1, events: [], bytes: 0, reason })).byteLength - 3;
  let bytes = arrayBytes + envelope + 1;
  while (arrayBytes + envelope + String(bytes).length !== bytes) bytes = arrayBytes + envelope + String(bytes).length;
  return bytes;
}

export function createRecordingBuffer(limits: { bytes: number; events: number; durationMs?: number } = RECORDING_LIMITS) {
  const events: eventWithTime[] = [];
  const encoder = new TextEncoder();
  let bytes = 2;
  let stopped: Recording | undefined;
  const stop = (reason: StopReason = 'manual'): Recording => {
    stopped ??= { version: 1, events: [...events], bytes: recordingBytes(bytes, reason), reason };
    return stopped;
  };
  return {
    append(event: eventWithTime): StopReason | undefined {
      if (stopped) return stopped.reason;
      if (events.length >= limits.events) return stop('events').reason;
      if (events.length && event.timestamp - events[0].timestamp > (limits.durationMs ?? RECORDING_LIMITS.durationMs)) return stop('duration').reason;
      const size = encoder.encode(JSON.stringify(event)).byteLength + (events.length ? 1 : 0);
      // Reserve the longest stop reason so every terminal envelope fits the same limit.
      if (recordingBytes(bytes + size, 'duration') > limits.bytes) return stop('bytes').reason;
      events.push(event);
      bytes += size;
      return undefined;
    },
    stop,
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
