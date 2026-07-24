import type { AcpCommandItemVm } from '@/types';

const SLASH_QUERY_RE = /^\/([\p{L}\p{N}._:-]*)$/u;
const LEADING_SLASH_COMMAND_RE = /^\/([\p{L}\p{N}._:-]+)/u;
const SLASH_COMMAND_SEPARATOR_RE = /^[\s\p{P}]/u;
const dismissedSlashQueries = new Map<string, string>();

export interface CommittedSlashCommand {
  command: AcpCommandItemVm;
  prefix: string;
  suffix: string;
}

export function matchSlashCommandQuery(input: string): string | null {
  const match = input.match(SLASH_QUERY_RE);
  return match?.[1] ?? null;
}

export function filterSlashCommands(
  commands: readonly AcpCommandItemVm[],
  query: string,
): AcpCommandItemVm[] {
  const keyword = query.trim().toLocaleLowerCase();
  if (!keyword) return [...commands];
  return commands.filter((command) =>
    command.name.toLocaleLowerCase().includes(keyword),
  );
}

export function slashCommandText(commandName: string): string {
  return `/${commandName.trim().replace(/^\/+/, '')} `;
}

export function parseCommittedSlashCommand(
  input: string,
  commands: readonly AcpCommandItemVm[],
): CommittedSlashCommand | null {
  const match = input.match(LEADING_SLASH_COMMAND_RE);
  if (!match) return null;
  const typedName = match[1];
  const prefixLength = typedName.length + 1;
  const suffix = input.slice(prefixLength);
  if (!suffix || !SLASH_COMMAND_SEPARATOR_RE.test(suffix)) return null;
  const command = commands.find(
    (candidate) => candidate.name.localeCompare(typedName, undefined, { sensitivity: 'accent' }) === 0,
  );
  if (!command) return null;
  return {
    command,
    prefix: input.slice(0, prefixLength),
    suffix,
  };
}

export function rememberSlashCommandDismissal(
  contextKey: string | null | undefined,
  input: string,
): void {
  if (contextKey) dismissedSlashQueries.set(contextKey, input);
}

export function restoreSlashCommandDismissal(
  contextKey: string | null | undefined,
  input: string,
  hasQuery: boolean,
): boolean {
  if (!contextKey) return false;
  const dismissedInput = dismissedSlashQueries.get(contextKey);
  if (!hasQuery || dismissedInput !== input) {
    dismissedSlashQueries.delete(contextKey);
    return false;
  }
  return true;
}

export function clearSlashCommandDismissal(
  contextKey: string | null | undefined,
): void {
  if (contextKey) dismissedSlashQueries.delete(contextKey);
}

export interface ActiveSlashCommandScrollInput {
  containerScrollTop: number;
  containerHeight: number;
  itemOffsetTop: number;
  itemOffsetHeight: number;
}

export function getScrollTopForActiveSlashCommand({
  containerScrollTop,
  containerHeight,
  itemOffsetTop,
  itemOffsetHeight,
}: ActiveSlashCommandScrollInput): number {
  const viewportBottom = containerScrollTop + containerHeight;
  const itemBottom = itemOffsetTop + itemOffsetHeight;
  if (itemOffsetTop < containerScrollTop) return itemOffsetTop;
  if (itemBottom > viewportBottom) return itemBottom - containerHeight;
  return containerScrollTop;
}
