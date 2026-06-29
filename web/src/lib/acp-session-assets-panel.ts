import type { AssetItemVm } from "@/types";

export type AcpSessionAssetPanelKind = "artifact" | "attachment";

export type AcpSessionAssetPanelItem = AssetItemVm & {
  kind: AcpSessionAssetPanelKind;
};

export interface AcpSessionAssetPanelViewModel {
  items: AcpSessionAssetPanelItem[];
  artifactCount: number;
  attachmentCount: number;
  totalCount: number;
  summaryParts: Array<{ kind: AcpSessionAssetPanelKind; count: number }>;
}

export function createAcpSessionAssetPanelViewModel(
  artifacts: AssetItemVm[],
  attachments: AssetItemVm[],
): AcpSessionAssetPanelViewModel {
  const artifactItems = artifacts.map((item) => ({
    ...item,
    kind: "artifact" as const,
  }));
  const attachmentItems = attachments.map((item) => ({
    ...item,
    kind: "attachment" as const,
  }));
  const items = [...artifactItems, ...attachmentItems];

  return {
    items,
    artifactCount: artifactItems.length,
    attachmentCount: attachmentItems.length,
    totalCount: items.length,
    summaryParts: [
      artifactItems.length > 0
        ? { kind: "artifact" as const, count: artifactItems.length }
        : null,
      attachmentItems.length > 0
        ? { kind: "attachment" as const, count: attachmentItems.length }
        : null,
    ].filter(
      (part): part is { kind: AcpSessionAssetPanelKind; count: number } =>
        part !== null,
    ),
  };
}
