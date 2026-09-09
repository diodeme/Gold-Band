import type { RuntimeApi } from '@/api/client';
import { createDemoApi } from '../demo/api';
import { createWorkflowSource } from './workflow-source';
import type { Language } from '../site/content';

// Overrides belong to this recording only. The shared preview and desktop adapters stay untouched.
export function createRecordingApi(options?: { scene: string; language: Language }): RuntimeApi & { scenario?: ReturnType<typeof createWorkflowSource> } {
  const readOnlyApi = createDemoApi();
  const scenario = options?.scene === 'during' ? createWorkflowSource(readOnlyApi, options.language) : undefined;
  return new Proxy({ ...scenario?.api, scenario } as RuntimeApi & { scenario?: ReturnType<typeof createWorkflowSource> }, {
    get(target, key) { return Reflect.get(target, key) ?? Reflect.get(readOnlyApi, key); },
  });
}
const options = typeof location === 'undefined' ? null : new URLSearchParams(location.search);
export const browserApi = createRecordingApi(options ? { scene: options.get('scene') ?? 'after', language: options.get('language') === 'en' ? 'en' : 'zh' } : undefined);
export const desktopApi = browserApi;
