import { describe, expect, it, vi } from 'vitest';
import { cameraTransform, checkpointAt, FULL_FRAME, playbackGate, readRecording, restoreCheckpoint, sampleTrack, type Shot } from '../../marketing/site/timeline';
import { settleReplayAnimations } from '../../marketing/site/replay-options';

const shots: Shot[] = [
  { id: 'start', at: 0, desktop: FULL_FRAME, mobile: FULL_FRAME, transition: 0, speed: 2 },
  { id: 'result', at: 1000, desktop: { x: 0.25, y: 0.25, width: 0.5, height: 0.5 }, mobile: { x: 0.5, y: 0.25, width: 0.25, height: 0.5 }, transition: 500, speed: 1 },
];
describe('recording clock and camera contract', () => {
  it('settles finite entrance animations during reconstruction without advancing infinite status animations', () => {
    const finite = { effect: { getComputedTiming: () => ({ endTime: 150 }) }, finish: vi.fn() };
    const infinite = { effect: { getComputedTiming: () => ({ endTime: Infinity }) }, finish: vi.fn() };
    const detached = { effect: null, finish: vi.fn() };
    settleReplayAnimations({ getAnimations: () => [finite, infinite, detached] } as unknown as Document);
    expect(finite.finish).toHaveBeenCalledOnce();
    expect(infinite.finish).not.toHaveBeenCalled();
    expect(detached.finish).not.toHaveBeenCalled();
    expect(() => settleReplayAnimations(null)).not.toThrow();
  });
  it('uses source time for smooth camera and speed transitions', () => {
    expect(sampleTrack(shots, 1000, false)).toEqual({ checkpoint: 'result', settled: false, frame: FULL_FRAME, speed: 2 });
    expect(sampleTrack(shots, 1250, false)).toEqual({ checkpoint: 'result', settled: false, frame: { x: 0.125, y: 0.125, width: 0.75, height: 0.75 }, speed: 1.5 });
    expect(sampleTrack(shots, 1500, true)).toMatchObject({ checkpoint: 'result', settled: true });
    expect(sampleTrack(shots, 1500, true).frame).toEqual(shots[1].mobile);
    expect(sampleTrack(shots, 1500, false).speed).toBe(1);
  });
  it('restores semantic progress across independently timed assets', () => {
    const checkpoint = checkpointAt(shots, 3000, 2000);
    expect(checkpoint).toEqual({ id: 'result', progress: 0.5 });
    expect(restoreCheckpoint([{ ...shots[0] }, { ...shots[1], at: 2000 }], 6000, checkpoint)).toBe(4000);
    expect(() => restoreCheckpoint(shots, 3000, { id: 'missing', progress: 0 })).toThrow();
  });
  it('frames the entire surface, including portals and the pointer', () => {
    expect(cameraTransform(FULL_FRAME, { width: 1440, height: 880 }, { width: 720, height: 440 })).toBe('translate(0px, 0px) scale(0.5)');
    expect(cameraTransform(shots[1].desktop, { width: 1440, height: 880 }, { width: 720, height: 440 })).toBe('translate(-360px, -220px) scale(1)');
  });
  it('pauses idempotently and resumes at the actual source clock', () => {
    const transport = { play: vi.fn(), pause: vi.fn(), getCurrentTime: vi.fn(() => 1250) };
    const gate = playbackGate(transport);
    gate(true); gate(true); gate(false); gate(false); gate(true);
    expect(transport.pause).toHaveBeenCalledTimes(1);
    expect(transport.play.mock.calls).toEqual([[1250], [1250]]);
  });
  it('rejects unknown versions, out-of-order or over-budget recordings', () => {
    const valid = { version: 1, reason: 'manual', bytes: 100, events: [{ type: 2, timestamp: 100, data: {} }, { type: 3, timestamp: 1000, data: {} }] };
    expect(readRecording(valid).duration).toBe(900);
    for (const invalid of [{ ...valid, version: 2 }, { ...valid, bytes: 13 * 1024 * 1024 }, { ...valid, reason: 'events' }, { ...valid, events: [...valid.events].reverse() }, { ...valid, events: [valid.events[0], { ...valid.events[1], timestamp: 60101 }] }]) {
      expect(() => readRecording(invalid)).toThrow();
    }
  });
});
