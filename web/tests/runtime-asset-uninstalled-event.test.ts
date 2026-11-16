import { describe, expect, it } from 'vitest';
import { RUNTIME_ASSET_UNINSTALLED_EVENT } from '../src/lib/app-events';
import type { RuntimeAssetUninstalledEventVm } from '../src/types';

// M5-cw 被动卸载事件契约（与 installed 对称）：事件名与载荷形状是跨端契约（Rust
// `src-tauri/src/multica/runtime_assets.rs` emit ↔ 此处订阅），任一侧改名此处先红。
// Rust 侧载荷经 `#[serde(rename_all = "camelCase")]` 序列化：assetType / uninstalledKey。
describe('runtime-asset-uninstalled event contract', () => {
  it('locks the cross-boundary event name', () => {
    expect(RUNTIME_ASSET_UNINSTALLED_EVENT).toBe('gold-band://runtime-asset-uninstalled');
  });

  it('payload shape is camelCase with assetType and uninstalledKey', () => {
    const payload: RuntimeAssetUninstalledEventVm = {
      assetType: 'role',
      uninstalledKey: 'pf-mcx1a2',
    };
    expect(payload.assetType).toBe('role');
    expect(payload.uninstalledKey).toBe('pf-mcx1a2');
    expect(Object.keys(payload).sort()).toEqual(['assetType', 'uninstalledKey']);
  });
});
