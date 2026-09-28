import type { AcpCommandItemVm, WorkspaceDirectoryEntryVm } from '@/types';

const SLASH_QUERY_RE = /^\/([\p{L}\p{N}._:-]*)$/u;
const MENTION_QUERY_RE = /^@([\p{L}\p{N}._:-]*)$/u;
const LEADING_COMPOSER_TOKEN_RE = /^([/@])([\p{L}\p{N}._:-]+)/u;
const SLASH_COMMAND_SEPARATOR_RE = /^[\s\p{P}]/u;
const MAX_VISIBLE_SLASH_COMMANDS = 512;
const dismissedSlashQueries = new Map<string, string>();

export interface CommittedSlashCommand {
  command: AcpCommandItemVm;
  prefix: string;
  suffix: string;
}

export type SlashCatalogItemKind =
  | 'role'
  | 'command'
  | 'mention-category'
  | 'workspace-directory'
  | 'workspace-file';

export interface SlashCatalogItem {
  kind: SlashCatalogItemKind;
  id: string;
  name: string;
  description: string;
  inputHint?: string;
  content?: string;
  profileName?: string;
  /** Workspace entry behind a `workspace-directory` / `workspace-file` item. */
  workspaceEntry?: WorkspaceDirectoryEntryVm;
}

export type SlashCatalogGroupStatus = 'loading' | 'empty' | 'error';

export interface SlashCatalogGroup {
  id: 'roles' | 'agent' | 'mention-categories' | 'workspace-files';
  heading: string;
  items: SlashCatalogItem[];
  /** Shown in place of items while a group's entries are loading, empty or failed. */
  status?: SlashCatalogGroupStatus;
}

export type MentionCategory = 'files' | 'roles';

/** Where the `@` menu is: the category list, the role list, or a workspace directory. */
export type MentionView =
  | { kind: 'root' }
  | { kind: 'roles' }
  | { kind: 'files'; path: string };

export const MENTION_ROOT_VIEW: MentionView = { kind: 'root' };

/** Workspace entries the `@` menu needs for its current view. */
export type MentionFilesRequest =
  | { kind: 'directory'; path: string }
  | { kind: 'search'; query: string };

export interface MentionFilesState {
  status: 'loading' | 'ready' | 'error';
  entries: readonly WorkspaceDirectoryEntryVm[];
}

export interface MentionMenuLabels {
  files: string;
  roles: string;
  workspaceRoot: string;
}

export function mentionFilesRequest(view: MentionView, query: string): MentionFilesRequest | null {
  const keyword = query.trim();
  if (view.kind === 'roles') return null;
  if (keyword) return { kind: 'search', query: keyword };
  return view.kind === 'files' ? { kind: 'directory', path: view.path } : null;
}

export function parentMentionView(view: MentionView): MentionView {
  if (view.kind !== 'files' || !view.path) return MENTION_ROOT_VIEW;
  const separator = view.path.lastIndexOf('/');
  return { kind: 'files', path: separator < 0 ? '' : view.path.slice(0, separator) };
}

function workspaceEntryItem(entry: WorkspaceDirectoryEntryVm): SlashCatalogItem | null {
  const path = entry.relativePath.replaceAll('\\', '/');
  if (entry.kind === 'directory') {
    return { kind: 'workspace-directory', id: path, name: entry.name, description: '', workspaceEntry: entry };
  }
  if (entry.kind !== 'file') return null;
  const separator = path.lastIndexOf('/');
  return {
    kind: 'workspace-file',
    id: path,
    name: entry.name,
    description: separator < 0 ? '' : path.slice(0, separator),
    workspaceEntry: entry,
  };
}

function workspaceFilesGroup(heading: string, files: MentionFilesState): SlashCatalogGroup {
  const items = files.entries.flatMap((entry) => workspaceEntryItem(entry) ?? []);
  // A refining search keeps showing the previous results until the new ones arrive.
  const status = files.status === 'error'
    ? 'error'
    : items.length > 0 ? undefined : files.status === 'loading' ? 'loading' : 'empty';
  return { id: 'workspace-files', heading, items, ...(status ? { status } : {}) };
}

/**
 * Groups of the `@` menu. With no query it navigates: categories, then roles or
 * workspace directories. A query searches every available category at once.
 */
export function buildMentionGroups({
  view,
  query,
  roleItems,
  filesAvailable,
  files,
  labels,
}: {
  view: MentionView;
  query: string;
  roleItems: readonly SlashCatalogItem[];
  filesAvailable: boolean;
  files: MentionFilesState;
  labels: MentionMenuLabels;
}): SlashCatalogGroup[] {
  const keyword = query.trim();
  const roles = (items: readonly SlashCatalogItem[], heading: string): SlashCatalogGroup[] => (
    items.length > 0 ? [{ id: 'roles', heading, items: [...items] }] : []
  );
  if (view.kind === 'roles') {
    return filterSlashCatalog(roles(roleItems, ''), keyword);
  }
  if (view.kind === 'files') {
    if (!filesAvailable) return [];
    return [workspaceFilesGroup(keyword ? labels.files : view.path || labels.workspaceRoot, files)];
  }
  if (keyword) {
    return [
      ...filterSlashCatalog(roles(roleItems, labels.roles), keyword),
      ...(filesAvailable ? [workspaceFilesGroup(labels.files, files)] : []),
    ];
  }
  const categories: SlashCatalogItem[] = [
    ...(filesAvailable ? [{ kind: 'mention-category' as const, id: 'files', name: labels.files, description: '' }] : []),
    ...(roleItems.length > 0 ? [{ kind: 'mention-category' as const, id: 'roles', name: labels.roles, description: '' }] : []),
  ];
  return categories.length > 0 ? [{ id: 'mention-categories', heading: '', items: categories }] : [];
}

export function slashCatalogGroupsHaveContent(groups: readonly SlashCatalogGroup[]): boolean {
  return groups.some((group) => group.items.length > 0 || group.status !== undefined);
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
    .replace(/^[@/]+/u, '')
    .replace(/[/\s]+/gu, '-')
    .replace(/[^\p{L}\p{N}._:-]+/gu, '-')
    .replace(/-+/gu, '-')
    .replace(/^-+|-+$/gu, '');
}

export function slashTagDisplayName(prefix: string): string {
  return prefix.replace(/^[@/]+/u, '');
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
  agentHeading: string,
  profiles: readonly { id: string; name: string; summary: string; content: string }[],
  commands: readonly AcpCommandItemVm[],
): SlashCatalogGroup[] {
  const groups: SlashCatalogGroup[] = [];
  const roles = roleSlashItems(profiles);
  if (roles.length > 0) {
    groups.push({ id: 'roles', heading: '', items: roles });
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
  const match = input.match(LEADING_COMPOSER_TOKEN_RE);
  if (!match) return null;
  const trigger = match[1];
  const typedName = match[2];
  const prefixLength = typedName.length + 1;
  const suffix = input.slice(prefixLength);
  if (!suffix || !SLASH_COMMAND_SEPARATOR_RE.test(suffix)) return null;
  const expectedKind: SlashCatalogItemKind = trigger === '@' ? 'role' : 'command';
  const matches = items.filter(
    (candidate) => candidate.kind === expectedKind
      && candidate.name.localeCompare(typedName, undefined, { sensitivity: 'accent' }) === 0,
  );
  if (matches.length === 0) return null;
  const selected = selectedIdentity
    ? matches.find((item) => item.kind === selectedIdentity.kind && item.id === selectedIdentity.id)
    : undefined;
  return {
    item: selected ?? matches[0],
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

export function composerTokenText(item: Pick<SlashCatalogItem, 'kind' | 'name'>): string {
  return item.kind === 'role' ? `@${item.name} ` : slashCommandText(item.name);
}

export function composerTextFromPromptRole(
  role: { name: string } | null | undefined,
  displayText = '',
): string {
  const name = role?.name.trim();
  if (!name) return displayText;
  return `${composerTokenText({ kind: 'role', name: slashTokenFromName(name) })}${displayText}`;
}

export type ComposerMenuTrigger = '/' | '@';

export function matchComposerMenuQuery(input: string): { trigger: ComposerMenuTrigger; query: string } | null {
  return matchComposerMenuQueryAt(input, input.length);
}

/**
 * The menu query is the `/` or `@` token typed at the start of the input, up to
 * the caret. Text after the caret is existing content that the chosen item is
 * inserted in front of.
 */
export function matchComposerMenuQueryAt(
  input: string,
  caret: number | null,
): { trigger: ComposerMenuTrigger; query: string } | null {
  if (caret === null) return null;
  const head = input.slice(0, caret);
  const mention = head.match(MENTION_QUERY_RE);
  if (mention) return { trigger: '@', query: mention[1] };
  const slash = head.match(SLASH_QUERY_RE);
  if (slash) return { trigger: '/', query: slash[1] };
  return null;
}

/** Replaces the menu query before `caret` and returns the new input with its caret. */
export function replaceComposerMenuQuery(
  input: string,
  caret: number,
  replacement: string,
): { input: string; caret: number } {
  const rest = input.slice(caret);
  // A completed token needs one separator before the text that follows it.
  const text = replacement.endsWith(' ') && /^\s/u.test(rest) ? replacement.trimEnd() : replacement;
  return { input: `${text}${rest}`, caret: text.length };
}

export function matchSlashCommandQuery(input: string): string | null {
  const match = matchComposerMenuQuery(input);
  return match?.trigger === '/' ? match.query : null;
}

export function groupsForComposerMenuTrigger(
  groups: readonly SlashCatalogGroup[],
  trigger: ComposerMenuTrigger,
): SlashCatalogGroup[] {
  return groups.filter((group) => (trigger === '@' ? group.id === 'roles' : group.id === 'agent'));
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
  return `/${commandName.trim().replace(/^[@/]+/, '')} `;
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
