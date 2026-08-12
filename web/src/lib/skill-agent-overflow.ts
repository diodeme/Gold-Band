const AGENT_ITEM_WIDTH = 24;
const AGENT_ITEM_GAP = 2;
const GROUPED_FIRST_SYNC_ITEM_WIDTH = 31;
const OVERFLOW_TRIGGER_WIDTH = 28;
const MAX_AGENT_ROWS = 2;

export interface SkillAgentOverflowLayout {
  visibleSourceCount: number;
  visibleSyncCount: number;
  hiddenCount: number;
}

function itemWidths(sourceCount: number, syncCount: number): number[] {
  const widths = Array.from({ length: sourceCount }, () => AGENT_ITEM_WIDTH);
  if (syncCount === 0) return widths;
  widths.push(sourceCount > 0 ? GROUPED_FIRST_SYNC_ITEM_WIDTH : AGENT_ITEM_WIDTH);
  widths.push(...Array.from({ length: syncCount - 1 }, () => AGENT_ITEM_WIDTH));
  return widths;
}

function fitsRows(widths: number[], availableWidth: number): boolean {
  if (widths.length === 0) return true;
  if (availableWidth <= 0) return false;

  let rows = 1;
  let rowWidth = 0;
  for (const width of widths) {
    const nextWidth = rowWidth === 0 ? width : rowWidth + AGENT_ITEM_GAP + width;
    if (nextWidth <= availableWidth) {
      rowWidth = nextWidth;
      continue;
    }
    rows += 1;
    if (rows > MAX_AGENT_ROWS || width > availableWidth) return false;
    rowWidth = width;
  }
  return true;
}

export function calculateSkillAgentOverflowLayout(
  availableWidth: number,
  sourceCount: number,
  syncCount: number,
): SkillAgentOverflowLayout {
  const safeSourceCount = Math.max(0, sourceCount);
  const safeSyncCount = Math.max(0, syncCount);
  const totalCount = safeSourceCount + safeSyncCount;

  if (fitsRows(itemWidths(safeSourceCount, safeSyncCount), availableWidth)) {
    return {
      visibleSourceCount: safeSourceCount,
      visibleSyncCount: safeSyncCount,
      hiddenCount: 0,
    };
  }

  for (let visibleCount = Math.max(0, totalCount - 1); visibleCount >= 0; visibleCount -= 1) {
    const visibleSourceCount = Math.min(safeSourceCount, visibleCount);
    const visibleSyncCount = Math.min(
      safeSyncCount,
      Math.max(0, visibleCount - visibleSourceCount),
    );
    const widths = [
      ...itemWidths(visibleSourceCount, visibleSyncCount),
      OVERFLOW_TRIGGER_WIDTH,
    ];
    if (fitsRows(widths, availableWidth)) {
      return {
        visibleSourceCount,
        visibleSyncCount,
        hiddenCount: totalCount - visibleSourceCount - visibleSyncCount,
      };
    }
  }

  return {
    visibleSourceCount: 0,
    visibleSyncCount: 0,
    hiddenCount: totalCount,
  };
}
