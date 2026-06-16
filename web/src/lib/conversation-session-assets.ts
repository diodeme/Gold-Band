import type { AssetItemVm, ConversationSessionLeafVm } from '@/types';

export function conversationAssetBelongsToLeaf(
  asset: AssetItemVm,
  leaf: ConversationSessionLeafVm | null | undefined,
) {
  if (!leaf) return false;
  return asset.roundId === leaf.roundId
    && asset.nodeId === leaf.nodeId
    && asset.attemptId === leaf.attemptId;
}

export function conversationAssetsForLeaf(
  assets: AssetItemVm[],
  leaf: ConversationSessionLeafVm | null | undefined,
) {
  return assets.filter((asset) => conversationAssetBelongsToLeaf(asset, leaf));
}
