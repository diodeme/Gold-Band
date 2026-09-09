import { z } from 'zod';

const milliseconds = z.number().finite().nonnegative();
const unit = z.number().finite().min(0).max(1);
const resource = z.string().min(1).refine((value) => {
  if (value.startsWith('//') || value.includes('\\')) return false;
  try { const url = new URL(value, 'https://assets.invalid/'); return url.origin === 'https://assets.invalid'; }
  catch { return false; }
});
const interval = { startMs: milliseconds, endMs: milliseconds };
export const SemanticCheckpointSchema = z.object({
  stepId: z.string().min(1), ...interval, poster: resource,
}).strict().refine((point) => point.endMs > point.startMs);
export const CameraKeyframeSchema = z.object({
  timeMs: milliseconds,
  rect: z.object({ x: unit, y: unit, width: unit.gt(0), height: unit.gt(0) }).strict()
    .refine((rect) => rect.x + rect.width <= 1 && rect.y + rect.height <= 1),
  zoom: z.number().finite().min(1).max(8),
  transitionMs: z.number().finite().min(0).max(600),
}).strict();
export const PaceSegmentSchema = z.object({
  ...interval, fromRate: z.number().finite().min(0.5).max(2), toRate: z.number().finite().min(0.5).max(2),
}).strict().refine((segment) => segment.endMs > segment.startMs);
export const SceneAssetSchema = z.object({
  version: z.literal(1),
  scene: z.enum(['before', 'during', 'after', 'personalize']),
  language: z.enum(['zh', 'en']), theme: z.enum(['dark', 'light']),
  rrwebVersion: z.literal('2.1.1'),
  eventsUrl: resource, sha256: z.string().regex(/^[a-f0-9]{64}$/),
  byteLength: z.number().int().positive().max(12 * 1024 * 1024),
  eventCount: z.number().int().min(2).max(12_000),
  durationMs: z.number().finite().positive().max(60_000),
  width: z.number().int().positive(), height: z.number().int().positive(),
  poster: resource,
  checkpoints: z.array(SemanticCheckpointSchema).min(1).max(64),
  camera: z.object({ desktop: z.array(CameraKeyframeSchema).min(1).max(128), mobile: z.array(CameraKeyframeSchema).min(1).max(128) }).strict(),
  pace: z.array(PaceSegmentSchema).min(1).max(128),
}).strict().superRefine((asset, context) => {
  const issue = (path: string) => context.addIssue({ code: 'custom', path: [path], message: 'site.asset-invalid' });
  for (const [name, segments] of [['checkpoints', asset.checkpoints], ['pace', asset.pace]] as const) {
    if (segments[0].startMs !== 0 || segments.at(-1)!.endMs !== asset.durationMs
      || segments.some((point, index) => index > 0 && point.startMs !== segments[index - 1].endMs)) issue(name);
  }
  if (new Set(asset.checkpoints.map((point) => point.stepId)).size !== asset.checkpoints.length) issue('checkpoints');
  for (const frames of Object.values(asset.camera)) {
    if (frames[0].timeMs !== 0 || frames.some((frame, index) => frame.timeMs > asset.durationMs
      || (index > 0 && frame.timeMs <= frames[index - 1].timeMs))) issue('camera');
  }
});
export const SceneManifestSchema = z.object({ version: z.literal(1), assets: z.array(SceneAssetSchema).min(1).max(16),
  mobileAssets: z.array(SceneAssetSchema).min(1).max(16).optional(),
}).strict()
  .superRefine(({ assets, mobileAssets = [] }, context) => {
    const keys = assets.map((asset) => `${asset.scene}/${asset.language}/${asset.theme}`);
    if (new Set(keys).size !== keys.length) context.addIssue({ code: 'custom', message: 'site.asset-invalid' });
    const mobileKeys = mobileAssets.map(asset => `${asset.scene}/${asset.language}/${asset.theme}`);
    if (new Set(mobileKeys).size !== mobileKeys.length) context.addIssue({ code: 'custom', message: 'site.asset-invalid' });
    for (const mobile of mobileAssets) {
      const primary = assets.find(asset => asset.scene === mobile.scene && asset.language === mobile.language && asset.theme === mobile.theme);
      if (!primary || primary.checkpoints.map(point => point.stepId).join('\0') !== mobile.checkpoints.map(point => point.stepId).join('\0')) {
        context.addIssue({ code: 'custom', message: 'site.checkpoint-mismatch' });
      }
    }
    for (const scene of new Set(assets.map((asset) => asset.scene))) {
      const variants = assets.filter((asset) => asset.scene === scene);
      const signature = variants[0].checkpoints.map((point) => point.stepId).join('\0');
      if (variants.some((asset) => asset.checkpoints.map((point) => point.stepId).join('\0') !== signature)) {
        context.addIssue({ code: 'custom', message: 'site.checkpoint-mismatch' });
      }
    }
  });
export type SceneAsset = z.infer<typeof SceneAssetSchema>;
export type SceneManifest = z.infer<typeof SceneManifestSchema>;
export const MOBILE_REPLAY_WIDTH = 600;
export function selectSceneAsset(manifest: SceneManifest, scene: SceneAsset['scene'], language: SceneAsset['language'], theme: SceneAsset['theme'], mobile: boolean) {
  const matches = (item: SceneAsset) => item.scene === scene && item.language === language && item.theme === theme;
  return (mobile ? manifest.mobileAssets?.find(matches) : undefined) ?? manifest.assets.find(matches);
}
export type SemanticCheckpoint = z.infer<typeof SemanticCheckpointSchema>;
export type CameraKeyframe = z.infer<typeof CameraKeyframeSchema>;
export type PaceSegment = z.infer<typeof PaceSegmentSchema>;
export type PlaybackPosition = { scene: SceneAsset['scene']; stepId: string; fraction: number };

export function playbackPosition(asset: SceneAsset, rawMs: number): PlaybackPosition {
  const time = Math.max(0, Math.min(asset.durationMs, rawMs));
  const point = asset.checkpoints.find((point) => time < point.endMs) ?? asset.checkpoints.at(-1)!;
  return { scene: asset.scene, stepId: point.stepId, fraction: (time - point.startMs) / (point.endMs - point.startMs) };
}

export function playbackTime(asset: SceneAsset, position: PlaybackPosition): number {
  const point = asset.checkpoints.find((point) => point.stepId === position.stepId);
  if (asset.scene !== position.scene || !point || !Number.isFinite(position.fraction) || position.fraction < 0 || position.fraction > 1) {
    throw { code: 'site.checkpoint-mismatch', params: { scene: position.scene, stepId: position.stepId } };
  }
  return point.startMs + position.fraction * (point.endMs - point.startMs);
}
