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
  if (selectionStart === null || selectionEnd === null || selectionStart !== selectionEnd) {
    return null;
  }
  const committed = parseCommittedSlashCommand(input, commands);
  if (!committed || committed.suffix !== ' ') return null;
  return committed.prefix;
}

/**
 * CC 内建的「会话内改配置」local 命令：由 CLI 本地执行、不经模型。其中 /model
 * 会重建 agent 侧工具集，导致 ACP session/new 注入的 MCP 服务器（如
 * gold-band-connections）从可用工具中消失（现场事故）。这类命令不能转发原始
 * 文本，须由产品路由到等价的原生设置（session/set_config_option）。
 */
const SESSION_MUTATING_LOCAL_SLASH_COMMANDS: ReadonlySet<string> = new Set(['model']);

export function isSessionMutatingLocalSlashCommand(name: string): boolean {
  return SESSION_MUTATING_LOCAL_SLASH_COMMANDS.has(name.toLowerCase());
}

/**
 * /model 是 CC 内建 local 命令（固定知识），识别不得依赖动态 agent 命令目录：
 * 目录加载失败或未收录时，`/model sonnet` 会原样转发给 agent 触发工具集重建
 * （现场事故 d596d1c9）。这里直接对原始输入做固定模式匹配。
 * @returns 命令参数（`/model` 后的剩余文本，已 trim）；非 /model 输入返回 null。
 */
export function parseModelSlashInput(input: string): string | null {
  const match = input.trimStart().match(/^\/model(?:\s+(.*))?$/i);
  if (!match) return null;
  return (match[1] ?? '').trim();
}

/**
 * 连接工具失联标记（连接→对话工具契约，见 src/prompts 下 connections/tool-usage.md）：
 * agent 在可用工具里找不到 gold-band-connections 工具时以该标记结束本轮。前端
 * 据此在时间线里派生恢复态并给出一键重试（后端已同步重注册工具）。
 */
export const CONNECTION_TOOLS_MISSING_MARKER = '[gold-band:connection-tools-missing]';

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
