import { playbackPosition, playbackTime, type CameraKeyframe, type PlaybackPosition, type SceneAsset } from './replay-model';

export interface ReplayEngine {
  getCurrentTime(): number;
  play(rawMs: number): void;
  pause(rawMs?: number): void;
  setConfig(config: { speed: number }): void;
  destroy(): void;
}
export type ReplayStatus = 'loading' | 'playing' | 'paused' | 'error' | 'disposed';
export type ReplayView = { status: ReplayStatus; poster: string; error?: { code: string } };
type Transform = { x: number; y: number; scale: number };
const clamp = (value: number, min: number, max: number) => Math.max(min, Math.min(max, value));
const mix = (a: number, b: number, fraction: number) => a + (b - a) * fraction;

export function paceAt(asset: SceneAsset, rawMs: number) {
  const segment = asset.pace.find(point => rawMs < point.endMs) ?? asset.pace.at(-1)!;
  return mix(segment.fromRate, segment.toRate, clamp((rawMs - segment.startMs) / (segment.endMs - segment.startMs), 0, 1));
}

function frameTransform(frame: CameraKeyframe, asset: SceneAsset, width: number, height: number): Transform {
  const scale = Math.min(width / (asset.width * frame.rect.width), height / (asset.height * frame.rect.height)) * frame.zoom;
  const x = width / 2 - (frame.rect.x + frame.rect.width / 2) * asset.width * scale;
  const y = height / 2 - (frame.rect.y + frame.rect.height / 2) * asset.height * scale;
  return {
    scale,
    x: scale * asset.width < width ? (width - scale * asset.width) / 2 : clamp(x, width - scale * asset.width, 0),
    y: scale * asset.height < height ? (height - scale * asset.height) / 2 : clamp(y, height - scale * asset.height, 0),
  };
}

export function cameraAt(asset: SceneAsset, rawMs: number, width: number, height: number, mobile: boolean, reduced: boolean): Transform {
  const frames = asset.camera[mobile ? 'mobile' : 'desktop'];
  let index = frames.findIndex(frame => frame.timeMs > rawMs) - 1;
  if (index === -2) index = frames.length - 1;
  index = Math.max(0, index);
  const frame = frames[index];
  const target = frameTransform(frame, asset, width, height);
  if (reduced || index === 0 || !frame.transitionMs) return target;
  const elapsed = clamp((rawMs - frame.timeMs) / frame.transitionMs, 0, 1);
  const fraction = elapsed * elapsed * (3 - 2 * elapsed);
  const previous = frameTransform(frames[index - 1], asset, width, height);
  return { x: mix(previous.x, target.x, fraction), y: mix(previous.y, target.y, fraction), scale: mix(previous.scale, target.scale, fraction) };
}

type Dependencies = {
  load: (asset: SceneAsset, signal: AbortSignal) => Promise<unknown>;
  create: (data: unknown, asset: SceneAsset) => ReplayEngine;
  requestFrame: (callback: () => void) => number;
  cancelFrame: (id: number) => void;
  paint: (asset: SceneAsset, rawMs: number, reduced: boolean) => void;
  changed: (view: ReplayView) => void;
};

// The engine owns elapsed time. This controller owns only intent and resource lifetime.
export class ReplayController {
  private engine?: ReplayEngine;
  private asset?: SceneAsset;
  private request?: AbortController;
  private frame = 0;
  private visible = true;
  private hidden = false;
  private reduced = false;
  private requested = true;
  private disposed = false;
  private status: ReplayStatus = 'paused';
  private position?: PlaybackPosition;
  constructor(private readonly dependencies: Dependencies) {}

  getPosition() { return this.engine && this.asset ? playbackPosition(this.asset, this.engine.getCurrentTime()) : this.position; }
  getRawTime() { return this.engine?.getCurrentTime() ?? 0; }
  private emit(status: ReplayStatus, error?: { code: string }) {
    this.status = status;
    const checkpoint = this.asset?.checkpoints.find(point => point.stepId === this.position?.stepId);
    this.dependencies.changed({ status, poster: checkpoint?.poster ?? this.asset?.poster ?? '', error });
  }
  private release() {
    this.dependencies.cancelFrame(this.frame); this.frame = 0;
    this.engine?.pause(); this.engine?.destroy(); this.engine = undefined;
  }
  suspend() {
    if (this.disposed) return;
    this.position = this.getPosition();
    this.request?.abort(); this.release(); this.emit('loading');
  }
  async select(asset: SceneAsset) {
    if (this.disposed) return;
    this.position = this.getPosition();
    this.request?.abort(); this.release();
    const request = new AbortController(); this.request = request; this.asset = asset;
    if (this.position?.scene !== asset.scene) this.position = undefined;
    this.emit('loading');
    try {
      const data = await this.dependencies.load(asset, request.signal);
      if (request.signal.aborted || this.disposed || this.request !== request) return;
      const rawMs = this.position ? playbackTime(asset, this.position) : 0;
      this.engine = this.dependencies.create(data, asset);
      this.engine.pause(rawMs);
      this.dependencies.paint(asset, rawMs, this.reduced);
      this.reconcile();
    } catch (error) {
      if (request.signal.aborted || this.disposed || this.request !== request) return;
      this.release();
      this.emit('error', { code: typeof error === 'object' && error && 'code' in error ? String(error.code) : 'site.replay-unavailable' });
    }
  }
  setEnvironment(options: { visible?: boolean; hidden?: boolean; reduced?: boolean }) {
    if (options.visible !== undefined) this.visible = options.visible;
    if (options.hidden !== undefined) this.hidden = options.hidden;
    if (options.reduced !== undefined && options.reduced !== this.reduced) {
      this.reduced = options.reduced;
      if (this.reduced) this.requested = false;
    }
    this.reconcile();
  }
  setPlaying(requested: boolean) { this.requested = requested; this.reconcile(); }
  seek(position: PlaybackPosition) {
    if (!this.engine || !this.asset) return;
    this.engine.pause(playbackTime(this.asset, position));
    this.position = position; this.status = 'paused'; this.reconcile(); this.paint();
  }
  paint() { if (this.engine && this.asset) this.dependencies.paint(this.asset, this.engine.getCurrentTime(), this.reduced); }
  private reconcile() {
    if (!this.engine || !this.asset || this.disposed) return;
    const playing = this.requested && this.visible && !this.hidden;
    if (playing && this.status !== 'playing') {
      const rawMs = this.engine.getCurrentTime() >= this.asset.durationMs ? 0 : this.engine.getCurrentTime();
      this.engine.setConfig({ speed: paceAt(this.asset, rawMs) });
      this.engine.play(rawMs); this.emit('playing'); this.tick();
    } else if (!playing) {
      this.engine.pause(); this.dependencies.cancelFrame(this.frame); this.frame = 0;
      this.emit('paused'); this.paint();
    }
  }
  private tick = () => {
    this.dependencies.cancelFrame(this.frame); this.frame = 0;
    if (!this.engine || !this.asset || this.status !== 'playing') return;
    const rawMs = this.engine.getCurrentTime();
    if (rawMs >= this.asset.durationMs) {
      this.engine.pause(this.asset.durationMs);
      this.requested = false; this.emit('paused'); this.paint(); return;
    }
    this.engine.setConfig({ speed: paceAt(this.asset, rawMs) });
    this.dependencies.paint(this.asset, rawMs, this.reduced);
    this.frame = this.dependencies.requestFrame(this.tick);
  };
  dispose() {
    if (this.disposed) return;
    this.disposed = true; this.request?.abort(); this.release(); this.emit('disposed');
  }
}
