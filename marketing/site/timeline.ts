import { z } from 'zod';
import type { Recording } from '../../web/rrweb-demo/recording';
import { RECORDING_LIMITS } from '../../web/rrweb-demo/recording';

const rectangle = z.object({ x: z.number().min(0).max(1), y: z.number().min(0).max(1), width: z.number().positive().max(1), height: z.number().positive().max(1) })
  .refine(r => r.x + r.width <= 1.001 && r.y + r.height <= 1.001);
export const shotSchema = z.object({
  id: z.string().min(1), at: z.number().nonnegative(),
  desktop: rectangle, mobile: rectangle,
  speed: z.number().min(0.8).max(2), transition: z.number().min(0).max(600),
});
export type Shot = z.infer<typeof shotSchema>;
export type Frame = Shot['desktop'];
export const FULL_FRAME: Frame = { x: 0, y: 0, width: 1, height: 1 };
const envelope = z.object({ version: z.literal(1), reason: z.literal('manual'), bytes: z.number().max(RECORDING_LIMITS.bytes), events: z.array(z.object({ type: z.number().int().min(0).max(6), timestamp: z.number().finite(), data: z.unknown() }).passthrough()).min(2).max(RECORDING_LIMITS.events) });

export function readRecording(value: unknown): { recording: Recording; shots: Shot[]; duration: number } {
  const parsed = envelope.parse(value);
  const first = parsed.events[0].timestamp;
  const duration = parsed.events.at(-1)!.timestamp - first;
  if (duration <= 0 || duration > RECORDING_LIMITS.durationMs || !parsed.events.some(e => e.type === 2)
    || parsed.events.some((e, i) => i > 0 && e.timestamp < parsed.events[i - 1].timestamp)) throw { code: 'site.recording-invalid' };
  const shots: Shot[] = [];
  for (const event of parsed.events) {
    const data = event.data as { tag?: string; payload?: unknown } | null;
    if (event.type === 5 && data?.tag === 'site-shot') shots.push(shotSchema.parse({ ...data.payload as object, at: event.timestamp - first }));
  }
  if (new Set(shots.map(s => s.id)).size !== shots.length) throw { code: 'site.duplicate-checkpoint' };
  if (!shots.length || shots[0].at > 0) shots.unshift({ id: 'establish', at: 0, desktop: FULL_FRAME, mobile: FULL_FRAME, speed: 1, transition: 0 });
  return { recording: parsed as Recording, shots, duration };
}
export function shotAt(shots: Shot[], time: number) {
  for (let i = shots.length - 1; i >= 0; i--) if (shots[i].at <= time) return shots[i];
  return shots[0];
}
export function checkpointAt(shots: Shot[], duration: number, time: number) {
  const shot = shotAt(shots, time);
  const end = shots[shots.indexOf(shot) + 1]?.at ?? duration;
  return { id: shot.id, progress: Math.min(1, Math.max(0, (time - shot.at) / Math.max(1, end - shot.at))) };
}
export function restoreCheckpoint(shots: Shot[], duration: number, checkpoint: { id: string; progress: number }) {
  const index = shots.findIndex(s => s.id === checkpoint.id);
  if (index < 0) throw { code: 'site.checkpoint-missing', params: { id: checkpoint.id } };
  return shots[index].at + Math.max(0, Math.min(1, checkpoint.progress)) * ((shots[index + 1]?.at ?? duration) - shots[index].at);
}
const ease = (t: number) => t * t * (3 - 2 * t);
export function sampleTrack(shots: Shot[], time: number, mobile: boolean) {
  const current = shotAt(shots, time);
  const previous = shots[Math.max(0, shots.indexOf(current) - 1)];
  const mix = ease(current.transition ? Math.min(1, Math.max(0, (time - current.at) / current.transition)) : 1);
  const from = mobile ? previous.mobile : previous.desktop;
  const to = mobile ? current.mobile : current.desktop;
  const lerp = (a: number, b: number) => a + (b - a) * mix;
  return { checkpoint: current.id, settled: mix === 1,
    frame: { x: lerp(from.x, to.x), y: lerp(from.y, to.y), width: lerp(from.width, to.width), height: lerp(from.height, to.height) }, speed: lerp(previous.speed, current.speed) };
}
export function cameraTransform(frame: Frame, source: { width: number; height: number }, stage: { width: number; height: number }) {
  const scale = Math.min(stage.width / (source.width * frame.width), stage.height / (source.height * frame.height));
  const x = stage.width / 2 - source.width * (frame.x + frame.width / 2) * scale;
  const y = stage.height / 2 - source.height * (frame.y + frame.height / 2) * scale;
  return `translate(${x}px, ${y}px) scale(${scale})`;
}
export interface ReplayTransport { play(time?: number): void; pause(): void; getCurrentTime(): number }
export function playbackGate(transport: ReplayTransport) {
  let running = false;
  return (allowed: boolean) => {
    if (allowed === running) return;
    running = allowed;
    if (running) transport.play(transport.getCurrentTime()); else transport.pause();
  };
}
