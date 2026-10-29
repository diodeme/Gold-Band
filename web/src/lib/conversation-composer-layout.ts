import type { CSSProperties } from 'react';

const COMPOSER_CONFIG_TRIGGER_SIZE_CLASS_NAME = 'h-8 px-2.5';
const COMPOSER_MODE_CONTROL_HEIGHT_CLASS_NAME = 'h-7';
const COMPOSER_TEXTAREA_BASE_CLASS_NAME = 'min-h-12 py-2 text-sm leading-6 text-foreground placeholder:text-muted-foreground';

export const COMPOSER_LEADING_ADORNMENT_SLOT_CLASS_NAME =
  'absolute z-10 inline-flex h-6 items-center';

export const COMPOSER_LEADING_ADORNMENT_CHIP_CLASS_NAME =
  'h-6 rounded-full border-border/70 bg-background/80 px-2 text-xs font-normal leading-none text-muted-foreground shadow-none hover:bg-muted/50 hover:text-foreground';

export const USER_MESSAGE_META_CHIP_CLASS_NAME =
  'h-7 rounded-full border-border/70 bg-background/80 px-2.5 text-xs font-normal leading-none text-muted-foreground shadow-none hover:bg-muted/50 hover:text-foreground';

export const ACP_SESSION_COMPOSER_BORDER_WIDTH_PX = 1;

export const ACP_SESSION_COMPOSER_BORDER_STYLE = {
  '--acp-session-composer-border-width': `${ACP_SESSION_COMPOSER_BORDER_WIDTH_PX}px`,
} as CSSProperties & { '--acp-session-composer-border-width': string };

export const CONVERSATION_HOME_COMPOSER_LAYOUT = {
  contentMaxWidthClassName: 'max-w-3xl',
  opticalBottomPaddingClassName: 'pb-[clamp(4rem,8vh,5rem)]',
  promptInputClassName: 'relative rounded-2xl border-border bg-card/60 px-2.5 py-2 shadow-sm',
  textareaClassName: `${COMPOSER_TEXTAREA_BASE_CLASS_NAME} w-full overflow-y-hidden px-0`,
  textareaMaxHeightPx: 320,
  containerClassName: '@container/conversation-composer flex flex-col gap-1.5',
  attachedInfoRailClassName: 'min-w-0',
  attachedInfoClassName: '@container/conversation-context relative mx-auto flex h-7 w-[80%] min-w-0 items-center justify-start gap-0 px-8 [--conversation-workspace-info-surface:var(--gold-surface-high)] shadow-none',
  toolbarClassName: 'mt-2 grid gap-1.5',
  simpleToolbarClassName: 'grid-cols-1 @xs/conversation-composer:grid-cols-[minmax(0,1fr)_auto] @xs/conversation-composer:items-center @xs/conversation-composer:gap-3',
  configuredToolbarClassName: 'grid-cols-1 @2xl/conversation-composer:grid-cols-[minmax(12rem,0.75fr)_minmax(28rem,1.25fr)] @2xl/conversation-composer:items-center @2xl/conversation-composer:gap-3',
  leadingActionsClassName: 'flex min-w-0 items-center gap-2',
  workspaceControlClassName: 'w-fit min-w-0 max-w-full flex-initial',
  simpleTrailingActionsClassName: 'flex min-w-0 justify-end',
  configuredTrailingActionsClassName: 'grid min-w-0 grid-cols-1 gap-2 @sm/conversation-composer:grid-cols-2 @lg/conversation-composer:grid-cols-[minmax(0,1fr)_minmax(0,1fr)_auto] @2xl/conversation-composer:grid-cols-[minmax(0,1fr)_minmax(0,1fr)_auto] @2xl/conversation-composer:items-center @2xl/conversation-composer:gap-3',
  configTriggerClassName: `${COMPOSER_CONFIG_TRIGGER_SIZE_CLASS_NAME} w-full max-w-none`,
  submitActionsClassName: 'flex w-fit min-w-0 shrink-0 justify-self-end @sm/conversation-composer:col-start-2 @lg/conversation-composer:col-start-3',
  sendButtonClassName: 'h-8 w-auto shrink-0 gap-1.5 rounded-full px-3',
  optionSectionClassName: 'flex flex-col items-stretch gap-2 rounded-xl border border-border/50 bg-card/40 px-4 py-1 @sm/conversation-composer:flex-row @sm/conversation-composer:items-center',
  modeControlHeightClassName: COMPOSER_MODE_CONTROL_HEIGHT_CLASS_NAME,
  optionTabsListClassName: `${COMPOSER_MODE_CONTROL_HEIGHT_CLASS_NAME} w-full @sm/conversation-composer:w-fit`,
  agentSectionClassName: 'flex min-h-10 flex-col items-stretch gap-1.5 rounded-xl border border-border/50 bg-card/40 px-4 py-0 @sm/conversation-composer:flex-row @sm/conversation-composer:items-center',
  agentTabsClassName: 'gold-scrollbar-hidden w-full min-w-0 overflow-x-auto overflow-y-hidden py-1 @sm/conversation-composer:w-auto @sm/conversation-composer:flex-1',
  agentTabsListClassName: 'h-8 w-max max-w-none',
  agentOptionClassName: 'h-8 min-w-8 gap-2 rounded-full border border-transparent px-2.5 data-[state=active]:border-primary/25 data-[state=active]:bg-primary/10',
  autoSectionClassName: 'space-y-1.5 rounded-xl border border-border/50 bg-card/40 px-4 py-1',
  autoGoalClassName: 'w-full min-h-9 resize-y rounded-md border border-border/60 bg-background/35 px-3 py-1.5 text-xs leading-5 text-foreground outline-none placeholder:text-muted-foreground focus-visible:border-primary/40 focus-visible:ring-2 focus-visible:ring-primary/10',
  workflowSectionClassName: 'flex items-center gap-3 rounded-xl border border-border/50 bg-card/40 px-4 py-1',
} as const;

export const ACP_SESSION_COMPOSER_LAYOUT = {
  stackSurfaceClassName: 'border border-border shadow-none [border-width:var(--acp-session-composer-border-width)]',
  promptInputClassName: 'px-0',
  textareaClassName: `${COMPOSER_TEXTAREA_BASE_CLASS_NAME} px-2.5`,
  commandBarClassName: 'mt-1 flex min-w-0 flex-wrap items-center gap-1.5 px-1 py-1',
  leadingActionsClassName: 'flex min-w-0 flex-1 items-center gap-1.5',
  trailingActionsClassName: 'ml-auto shrink-0 gap-1.5 pl-1',
  configTriggerClassName: COMPOSER_CONFIG_TRIGGER_SIZE_CLASS_NAME,
  staticConfigClassName: 'h-8 px-2.5 py-0',
  actionButtonClassName: 'h-8 gap-1.5 rounded-full px-3',
  runningAgentsMaxHeightClassName: 'max-h-[4.75rem]',
} as const;

/** Layers above the session composer, top to bottom. */
export const ACP_COMPOSER_STACK_ORDER = ['agents', 'manual', 'todo', 'queue', 'composer'] as const;

export type AcpComposerStackLayer = typeof ACP_COMPOSER_STACK_ORDER[number];

/** A detached layer keeps a gap instead of joining its neighbours. */
const ACP_COMPOSER_DETACHED_LAYERS: ReadonlySet<AcpComposerStackLayer> = new Set(['manual']);

/** A seamless layer joins the layer below it without a divider line. */
const ACP_COMPOSER_SEAMLESS_LAYERS: ReadonlySet<AcpComposerStackLayer> = new Set(['agents']);

export type AcpComposerStackJoin = 'none' | 'divided' | 'seamless';

export interface AcpComposerStackSlot {
  above: AcpComposerStackJoin;
  below: AcpComposerStackJoin;
  /** The session info tab sits on this layer's top-left corner. */
  infoTab: boolean;
  /** A following layer exists but does not join this one. */
  gapBelow: boolean;
}

export type AcpComposerStackLayout = Partial<Record<AcpComposerStackLayer, AcpComposerStackSlot>>;

export function acpComposerStackLayout(
  visible: Partial<Record<AcpComposerStackLayer, boolean>>,
  infoTabVisible: boolean,
): AcpComposerStackLayout {
  const layers = ACP_COMPOSER_STACK_ORDER.filter((layer) => visible[layer]);
  const join = (upper: AcpComposerStackLayer | undefined, lower: AcpComposerStackLayer | undefined): AcpComposerStackJoin => {
    if (!upper || !lower) return 'none';
    if (ACP_COMPOSER_DETACHED_LAYERS.has(upper) || ACP_COMPOSER_DETACHED_LAYERS.has(lower)) return 'none';
    return ACP_COMPOSER_SEAMLESS_LAYERS.has(upper) ? 'seamless' : 'divided';
  };
  const layout: AcpComposerStackLayout = {};
  layers.forEach((layer, index) => {
    const below = join(layer, layers[index + 1]);
    layout[layer] = {
      above: join(layers[index - 1], layer),
      below,
      infoTab: infoTabVisible && index === 0,
      gapBelow: below === 'none' && index < layers.length - 1,
    };
  });
  return layout;
}

/** A standalone surface when the layer is rendered outside a stack. */
const ACP_COMPOSER_STANDALONE_SLOT: AcpComposerStackSlot = {
  above: 'none',
  below: 'none',
  infoTab: false,
  gapBelow: false,
};

export function acpComposerStackShapeClassName(slot: AcpComposerStackSlot = ACP_COMPOSER_STANDALONE_SLOT) {
  return [
    slot.above !== 'none' ? 'rounded-t-none' : slot.infoTab ? 'rounded-tl-none rounded-tr-2xl' : 'rounded-t-2xl',
    slot.below !== 'none' ? 'rounded-b-none' : 'rounded-b-2xl',
    slot.above === 'seamless' && 'border-t-0',
    slot.below === 'seamless' && 'border-b-0',
    slot.gapBelow && 'mb-3',
  ].filter(Boolean).join(' ');
}
