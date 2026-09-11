import type { eventWithTime } from '@rrweb/types';
import { sha256Hex } from '@/lib/sha256';
import { SceneAssetSchema, SceneManifestSchema, type SceneAsset } from './replay-model';

export async function loadSceneEvents(asset: SceneAsset, signal: AbortSignal): Promise<eventWithTime[]> {
  SceneAssetSchema.parse(asset);
  const response = await fetch(asset.eventsUrl, { signal });
  if (!response.ok) throw { code: 'site.replay-unavailable' };
  const buffer = await response.arrayBuffer();
  if (buffer.byteLength !== asset.byteLength) throw { code: 'site.asset-size' };
  const hash = await sha256Hex(buffer);
  if (hash !== asset.sha256) throw { code: 'site.asset-hash' };
  const recording = JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(buffer));
  const events = recording.events as eventWithTime[];
  if (recording.version !== 1 || recording.reason !== 'manual' || recording.bytes !== buffer.byteLength ||
    !Array.isArray(events) || events.length !== asset.eventCount ||
    events.some((event, index) => !Number.isFinite(event.timestamp) || (index > 0 && event.timestamp < events[index - 1].timestamp)) ||
    events.at(-1)!.timestamp - events[0].timestamp !== asset.durationMs) throw { code: 'site.asset-invalid' };
  return events;
}

export async function loadSceneManifest(url: string, signal: AbortSignal) {
  const response = await fetch(url, { signal });
  if (!response.ok) throw { code: 'site.replay-unavailable' };
  return SceneManifestSchema.parse(await response.json());
}
