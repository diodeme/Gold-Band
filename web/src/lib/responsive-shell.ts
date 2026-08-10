export const COMPACT_SHELL_MAX_WIDTH = 767;

export function shouldCollapseShellSidebar(width: number) {
  return width <= COMPACT_SHELL_MAX_WIDTH;
}
