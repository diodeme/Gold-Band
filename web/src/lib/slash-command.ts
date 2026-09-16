import type { AcpCommandItemVm } from '@/types';

const SLASH_QUERY_RE = /^\/([\p{L}\p{N}._:-]*)$/u;
const LEADING_SLASH_COMMAND_RE = /^\/([\p{L}\p{N}._:-]+)/u;
const SLASH_COMMAND_SEPARATOR_RE = /^[\s\p{P}]/u;
const MAX_VISIBLE_SLASH_COMMANDS = 512;
const dismissedSlashQueries = new Map<string, string>();

export interface SlashCommandFocusTarget {
  disabled?: boolean;
  value?: string;
  focus: () => void;
  setSelectionRange?: (start: number, end: number) => void;
}

export interface SlashCommandFocusRef {
  readonly current: SlashCommandFocusTarget | null;
}

export type SlashCommandFocusScheduler = (callback: () => void) => unknown;

export interface CommittedSlashCommand {
  command: AcpCommandItemVm;
  prefix: string;
  suffix: string;
}

export type SlashCatalogItemKind = 'role' | 'command';

export interface SlashCatalogItem {
  kind: SlashCatalogItemKind;
  id: string;
  name: string;
  description: string;
  inputHint?: string;
  content?: string;
  profileName?: string;
}

export interface SlashCatalogGroup {
  id: 'product' | 'agent';
  heading: string;
  items: SlashCatalogItem[];
}

export interface SlashItemIdentity {
  kind: SlashCatalogItemKind;
  id: string;
}

export interface CommittedSlashItem {
  item: SlashCatalogItem;
  prefix: string;
  suffix: string;
}

export function slashCatalogItemValue(item: SlashCatalogItem): string {
  return `${item.kind}:${item.id}`;
}

export function slashTokenFromName(name: string): string {
  return name
    .trim()
    .replace(/^\/+/u, '')
    .replace(/[/\s]+/gu, '-')
    .replace(/[^\p{L}\p{N}._:-]+/gu, '-')
    .replace(/-+/gu, '-')
    .replace(/^-+|-+$/gu, '');
}

export function roleSlashItems(
  profiles: readonly { id: string; name: string; summary: string; content: string }[],
): SlashCatalogItem[] {
  return profiles.flatMap((profile) => {
    const name = slashTokenFromName(profile.name);
    if (!name) return [];
    return [{
      kind: 'role' as const,
      id: profile.id,
      name,
      description: profile.summary,
      content: profile.content,
      profileName: profile.name,
    }];
  });
}

export function commandSlashItems(commands: readonly AcpCommandItemVm[]): SlashCatalogItem[] {
  return commands.map((command) => ({
    kind: 'command' as const,
    id: command.name,
    name: command.name,
    description: command.description,
    ...(command.inputHint ? { inputHint: command.inputHint } : {}),
  }));
}

export function buildSlashCatalog(
  productHeading: string,
  agentHeading: string,
  profiles: readonly { id: string; name: string; summary: string; content: string }[],
  commands: readonly AcpCommandItemVm[],
): SlashCatalogGroup[] {
  const groups: SlashCatalogGroup[] = [];
  const roles = roleSlashItems(profiles);
  if (roles.length > 0) {
    groups.push({ id: 'product', heading: productHeading, items: roles });
  }
  const commandItems = commandSlashItems(commands);
  if (commandItems.length > 0) {
    groups.push({ id: 'agent', heading: agentHeading, items: commandItems });
  }
  return groups;
}

export function flattenSlashCatalog(groups: readonly SlashCatalogGroup[]): SlashCatalogItem[] {
  return groups.flatMap((group) => group.items);
}

export function filterSlashCatalog(
  groups: readonly SlashCatalogGroup[],
  query: string,
): SlashCatalogGroup[] {
  const keyword = query.trim().toLocaleLowerCase();
  return groups.flatMap((group) => {
    const items = keyword
      ? group.items.filter((item) => item.name.toLocaleLowerCase().includes(keyword))
      : group.items;
    return items.length > 0 ? [{ ...group, items: [...items] }] : [];
  });
}

export function parseCommittedSlashItem(
  input: string,
  items: readonly SlashCatalogItem[],
  selectedIdentity: SlashItemIdentity | null = null,
): CommittedSlashItem | null {
  const match = input.match(LEADING_SLASH_COMMAND_RE);
  if (!match) return null;
  const typedName = match[1];
  const prefixLength = typedName.length + 1;
  const suffix = input.slice(prefixLength);
  if (!suffix || !SLASH_COMMAND_SEPARATOR_RE.test(suffix)) return null;
  const matches = items.filter(
    (candidate) => candidate.name.localeCompare(typedName, undefined, { sensitivity: 'accent' }) === 0,
  );
  if (matches.length === 0) return null;
  const selected = selectedIdentity
    ? matches.find((item) => item.kind === selectedIdentity.kind && item.id === selectedIdentity.id)
    : undefined;
  const item = selected
    ?? matches.find((candidate) => candidate.kind === 'role')
    ?? matches[0];
  return {
    item,
    prefix: input.slice(0, prefixLength),
    suffix,
  };
}

export function unwrapSelectedSlashItem(
  input: string,
  items: readonly SlashCatalogItem[],
  selectionStart: number | null,
  selectionEnd: number | null,
  selectedIdentity: SlashItemIdentity | null = null,
): string | null {
  if (selectionStart === null || selectionEnd === null || selectionStart !== selectionEnd) {
    return null;
  }
  const committed = parseCommittedSlashItem(input, items, selectedIdentity);
  if (!committed || committed.suffix !== ' ') return null;
  return committed.prefix;
}

export function committedRoleSnapshot(committed: CommittedSlashItem | null): {
  profileId: string;
  name: string;
  content: string;
} | null {
  if (committed?.item.kind !== 'role' || !committed.item.content?.trim()) return null;
  return {
    profileId: committed.item.id,
    name: committed.item.profileName ?? committed.item.name,
    content: committed.item.content,
  };
}

export function slashSendableText(input: string, committed: CommittedSlashItem | null): string {
  return committed?.item.kind === 'role' ? committed.suffix : input;
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

export function mergeSlashCommandSources(
  preferred: readonly unknown[] | null | undefined,
  fallback: readonly unknown[],
): AcpCommandItemVm[] {
  const merged: AcpCommandItemVm[] = [];
  const names = new Set<string>();
  for (const candidate of [...(preferred ?? []), ...fallback]) {
    const command = normalizeSlashCommand(candidate);
    if (!command) continue;
    const name = command.name.trim().replace(/^\/+/, '');
    const normalizedName = name.toLocaleLowerCase();
    if (!name || names.has(normalizedName)) continue;
    names.add(normalizedName);
    merged.push(name === command.name ? command : { ...command, name });
    if (merged.length >= MAX_VISIBLE_SLASH_COMMANDS) break;
  }
  return merged;
}

function normalizeSlashCommand(candidate: unknown): AcpCommandItemVm | null {
  if (!candidate || typeof candidate !== 'object') return null;
  const value = candidate as Record<string, unknown>;
  if (typeof value.name !== 'string') return null;
  return {
    name: value.name,
    description: typeof value.description === 'string' ? value.description : '',
    ...(typeof value.inputHint === 'string' ? { inputHint: value.inputHint } : {}),
  };
}

export function slashCommandText(commandName: string): string {
  return `/${commandName.trim().replace(/^\/+/, '')} `;
}

export function restoreSlashCommandInputFocus(
  inputRef: SlashCommandFocusRef,
  schedule: SlashCommandFocusScheduler = requestAnimationFrame,
): void {
  schedule(() => {
    const input = inputRef.current;
    if (!input || input.disabled) return;
    input.focus();
    if (typeof input.value === 'string' && input.setSelectionRange) {
      const caret = input.value.length;
      input.setSelectionRange(caret, caret);
    }
  });
}

export function unwrapSelectedSlashCommand(
  input: string,
  commands: readonly AcpCommandItemVm[],
  selectionStart: number | null,
  selectionEnd: number | null,
): string | null {
  return unwrapSelectedSlashItem(
    input,
    commandSlashItems(commands),
    selectionStart,
    selectionEnd,
  );
}

export function parseCommittedSlashCommand(
  input: string,
  commands: readonly AcpCommandItemVm[],
): CommittedSlashCommand | null {
  const committed = parseCommittedSlashItem(input, commandSlashItems(commands));
  if (!committed) return null;
  return {
    command: {
      name: committed.item.name,
      description: committed.item.description,
      ...(committed.item.inputHint ? { inputHint: committed.item.inputHint } : {}),
    },
    prefix: committed.prefix,
    suffix: committed.suffix,
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
