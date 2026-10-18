import { useId, useRef, useState, type KeyboardEvent, type MouseEvent, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { ChevronDown } from 'lucide-react';

import type { AcpModeVm, AcpSelectConfigOptionVm } from '@/types';
import { cn } from '@/lib/utils';
import { AcpModelConfigDiscoveryMenuSection, useAcpModelConfigDiscovery } from './AcpModelConfigDiscovery';
import { hasObservedAuthoringModel } from '@/lib/acp-composite-config';
import {
  ACP_COMPOSER_CONFIG_TRIGGER_ICON_CLASS,
  ACP_COMPOSER_CONFIG_TRIGGER_LABEL_CLASS,
  ACP_COMPOSER_CONFIG_TRIGGER_VALUE_CLASS,
  ACP_COMPOSER_CONFIG_DROPDOWN_MODAL,
  DEFAULT_ACP_COMPOSER_CONFIG_ALIGN,
  keepAcpConfigMenuOpenOnSelect,
  acpComposerConfigTriggerVariants,
  formatAcpCompositeSelectionParts,
  useAcpComposerConfigOverflowTooltip,
} from '@/components/acp/AcpComposerConfigTrigger';
import {
  AcpSingleConfigMenu,
  UNSPECIFIED_ACP_CONFIG_VALUE,
} from '@/components/acp/AcpSingleConfigMenu';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSub,
  DropdownMenuSubContent,
  DropdownMenuSubTrigger,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { overlayCollisionRect, overlayFitsBeside, useOverlayPositioning } from '@/lib/portal-container';
import {
  ACP_THOUGHT_LEVEL_CATEGORY,
  type AcpCompositeConfigSection,
  acpCompositeConfigSections,
  acpCompositeSectionLabel,
} from '@/lib/acp-composite-config';

export type { AcpCompositeConfigSection } from '@/lib/acp-composite-config';
export { UNSPECIFIED_ACP_CONFIG_VALUE } from '@/components/acp/AcpSingleConfigMenu';
export { formatAcpCompositeSelection, formatAcpCompositeSelectionParts } from '@/components/acp/AcpComposerConfigTrigger';
export {
  ACP_MODEL_CATEGORY,
  ACP_MODEL_CONFIG_CATEGORY,
  ACP_THOUGHT_LEVEL_CATEGORY,
  acpCompositeConfigSections,
  acpCompositeSectionLabel,
  acpShowsModelConfigSelect,
  authoringConfigOptionsForModel,
  isAuthoringConfigOptionValueAllowed,
  isAuthoringModelBoundConfigOption,
  findAcpCatalogModelId,
  findAcpModelConfigOptions,
  findAcpThoughtLevel,
  remapAcpThoughtLevelOverride,
  retainAcpModelBoundOverrides,
  rememberAcpModelBoundOverrides,
  switchAcpModelBoundOverrides,
  optionalAcpModelBoundOverrides,
} from '@/lib/acp-composite-config';

export function updateAcpConfigOptionOverride(
  overrides: Record<string, string> | null | undefined,
  optionId: string,
  value: string | null,
): Record<string, string> {
  const next = { ...(overrides ?? {}) };
  if (value) next[optionId] = value;
  else delete next[optionId];
  return next;
}

export function acpConfigMenuSelectionMode(
  thoughtLevel: AcpSelectConfigOptionVm | null | undefined,
  extraSections: Array<{ options: { length: number } }> | null | undefined = null,
) {
  const hasThought = Boolean(thoughtLevel && thoughtLevel.options.length > 0);
  const hasExtra = Boolean(extraSections?.some((section) => section.options.length > 0));
  return hasThought || hasExtra ? 'composite' : 'single';
}

/** Side flyout when the conversation pane has room beside the menu, otherwise expanded inside it. */
export type AcpCompositeSectionPresentation = 'flyout' | 'inline';
export type AcpCompositeOpenSection = { id: string; presentation: AcpCompositeSectionPresentation };

export function nextAcpCompositeSection(
  currentSection: AcpCompositeOpenSection | null,
  section: string,
  open: boolean,
  presentation: AcpCompositeSectionPresentation = 'flyout',
): AcpCompositeOpenSection | null {
  if (open) return { id: section, presentation };
  return currentSection?.id === section ? null : currentSection;
}

// Keep in sync with COMPOSITE_SUBMENU_CLASS width.
const COMPOSITE_SUBMENU_WIDTH_REM = 22;
const COMPOSITE_SUBMENU_SIDE_OFFSET = 6;
const COMPOSITE_SUBMENU_CLASS = 'w-[min(22rem,calc(100vw-2rem))] max-h-[min(24rem,var(--radix-dropdown-menu-content-available-height))] overflow-y-auto';
const COMPOSITE_SUBMENU_OPEN_KEYS = ['Enter', ' ', 'ArrowRight'];

type ThoughtLevelProps = Omit<AcpSelectConfigOptionVm, "options"> & {
  options: Array<AcpSelectConfigOptionVm["options"][number] & { available?: boolean }>;
};

type Props = {
  /** Enables on-demand model config discovery for this Agent. */
  agentType?: string;
  /** Callers projecting compositeSections supply this; otherwise derived from configOptions/modelBoundCatalogs. */
  modelConfigObserved?: boolean;
  models: Array<AcpModeVm & { available?: boolean }>;
  modelValue?: string | null;
  modelValueLabel?: string | null;
  configOptions?: AcpSelectConfigOptionVm[] | null;
  modelBoundCatalogs?: Record<string, AcpSelectConfigOptionVm[]> | null;
  configOptionValues?: Record<string, string> | null;
  compositeSections?: AcpCompositeConfigSection[] | null;
  thoughtLevel?: ThoughtLevelProps | null;
  thoughtValue?: string | null;
  thoughtValueLabel?: string | null;
  onModelChange: (value: string | null) => void;
  onConfigOptionChange?: (optionId: string, value: string | null) => void;
  onThoughtChange?: (optionId: string, value: string | null) => void;
  showUnspecifiedModel?: boolean;
  showUnspecifiedThought?: boolean;
  compact?: boolean;
  contentSide?: 'top' | 'bottom';
  align?: 'start' | 'end';
  triggerClassName?: string;
  disabled?: boolean;
};

export function AcpModelThoughtSelects(props: Props) {
  const { agentType, modelValue, configOptions, modelBoundCatalogs, modelConfigObserved } = props;
  // Owned here so the menu stays open when a fetched catalog switches single -> composite mode.
  const [menuOpen, setMenuOpen] = useState(false);
  const discovery = useAcpModelConfigDiscovery(agentType, modelValue);
  const observed = modelConfigObserved
    ?? hasObservedAuthoringModel(configOptions, modelBoundCatalogs, modelValue);
  const showDiscovery = Boolean(agentType && modelValue && !observed);
  return <AcpModelThoughtSelectsControl
    {...props}
    menuOpen={menuOpen}
    onMenuOpenChange={setMenuOpen}
    menuFooter={showDiscovery ? <AcpModelConfigDiscoveryMenuSection discovery={discovery} /> : null}
  />;
}

type MenuControlProps = {
  menuOpen: boolean;
  onMenuOpenChange: (open: boolean) => void;
  menuFooter: ReactNode;
};

function AcpModelThoughtSelectsControl({
  models,
  modelValue,
  modelValueLabel,
  configOptions,
  modelBoundCatalogs,
  configOptionValues,
  compositeSections,
  thoughtLevel,
  thoughtValue,
  thoughtValueLabel,
  onModelChange,
  onConfigOptionChange,
  onThoughtChange,
  showUnspecifiedModel = true,
  showUnspecifiedThought = true,
  compact = false,
  contentSide = 'bottom',
  align = DEFAULT_ACP_COMPOSER_CONFIG_ALIGN,
  triggerClassName,
  disabled = false,
  menuOpen,
  onMenuOpenChange: setMenuOpen,
  menuFooter,
}: Props & MenuControlProps) {
  const { t } = useTranslation();
  const [openSection, setOpenSection] = useState<AcpCompositeOpenSection | null>(null);
  const changeOpenSection = (id: string, open: boolean, presentation?: AcpCompositeSectionPresentation) => {
    setOpenSection((current) => nextAcpCompositeSection(current, id, open, presentation));
  };
  const keepMenuOpenRef = useRef(false);
  const triggerClass = acpComposerConfigTriggerVariants({ compact });
  const selectedModel = models.find((model) => model.id === modelValue);
  const changeConfigOption = onConfigOptionChange ?? onThoughtChange;
  const sections = resolveCompositeSections({
    compositeSections,
    configOptions,
    configOptionValues,
    modelValue,
    thoughtLevel,
    thoughtValue,
    thoughtValueLabel,
    showUnspecifiedThought,
    modelBoundCatalogs,
  });
  const thoughtLevelCount = sections.filter((section) => section.category === ACP_THOUGHT_LEVEL_CATEGORY).length;
  const selectionMode = sections.some((section) => section.options.length > 0) ? 'composite' : 'single';
  const {
    valueRef,
    tooltipOpen,
    handleTooltipFocus,
    hideTooltip,
    handleTooltipOpenChange,
  } = useAcpComposerConfigOverflowTooltip();
  const handleConfigOptionSelect = (event: Event) => {
    keepAcpConfigMenuOpenOnSelect(event);
    keepMenuOpenRef.current = true;
    setTimeout(() => {
      keepMenuOpenRef.current = false;
    }, 0);
  };
  const handleMenuOpenChange = (open: boolean) => {
    if (!open && keepMenuOpenRef.current) {
      keepMenuOpenRef.current = false;
      setMenuOpen(true);
      return;
    }
    setMenuOpen(open);
    if (!open) setOpenSection(null);
  };

  if (selectionMode === 'single') {
    return models.length > 0 ? (
      <AcpSingleConfigMenu
        label={t('acp.currentModel')}
        value={modelValue}
        valueLabel={modelValueLabel}
        options={models}
        unspecifiedLabel={t('conversation.home.unspecifiedModel')}
        onValueChange={onModelChange}
        showUnspecified={showUnspecifiedModel}
        compact={compact}
        contentSide={contentSide}
        align={align}
        triggerClassName={triggerClassName}
        disabled={disabled}
        open={menuOpen}
        onOpenChange={setMenuOpen}
        footer={menuFooter}
      />
    ) : null;
  }

  const unspecifiedLabel = t('conversation.home.unspecifiedModel');
  const compositeLabel = formatAcpCompositeSelectionParts(
    [
      selectedModel?.name ?? modelValueLabel,
      ...sections.map((section) => (
        section.options.find((option) => option.value === section.value)?.name
        ?? section.valueLabel
        ?? null
      )),
    ],
    unspecifiedLabel,
  );

  return (
    <DropdownMenu
      open={menuOpen}
      modal={ACP_COMPOSER_CONFIG_DROPDOWN_MODAL}
      onOpenChange={handleMenuOpenChange}
    >
      <Tooltip open={tooltipOpen} onOpenChange={handleTooltipOpenChange}>
        <TooltipTrigger asChild>
          <DropdownMenuTrigger
            disabled={disabled}
            className={cn(triggerClass, triggerClassName)}
            onPointerLeave={hideTooltip}
            onPointerDown={hideTooltip}
            onFocus={handleTooltipFocus}
            onBlur={hideTooltip}
          >
            <span className={ACP_COMPOSER_CONFIG_TRIGGER_LABEL_CLASS}>{t('acp.currentModel')}</span>
            <span
              ref={valueRef}
              className={ACP_COMPOSER_CONFIG_TRIGGER_VALUE_CLASS}
              data-acp-config-value="true"
            >
              {compositeLabel}
            </span>
            <ChevronDown className={ACP_COMPOSER_CONFIG_TRIGGER_ICON_CLASS} />
          </DropdownMenuTrigger>
        </TooltipTrigger>
        <TooltipContent
          side="top"
          sideOffset={6}
          className="max-w-[min(24rem,calc(100vw-2rem))] whitespace-normal break-words"
        >
          {compositeLabel}
        </TooltipContent>
      </Tooltip>
      <DropdownMenuContent
        side={contentSide}
        sideOffset={8}
        align={align}
        className="w-[min(19rem,calc(100vw-2rem))]"
      >
        <CompositeConfigSection
          id="model"
          label={t('acp.currentModel')}
          labelClassName="w-20"
          valueLabel={selectedModel?.name ?? modelValueLabel ?? ''}
          value={modelValue || UNSPECIFIED_ACP_CONFIG_VALUE}
          onValueChange={(value) => onModelChange(value === UNSPECIFIED_ACP_CONFIG_VALUE ? null : value)}
          unspecifiedLabel={showUnspecifiedModel ? unspecifiedLabel : null}
          items={models.map((model) => ({
            value: model.id,
            name: model.name,
            description: model.description,
            available: model.available,
          }))}
          openSection={openSection}
          onOpenChange={changeOpenSection}
          onItemSelect={handleConfigOptionSelect}
        />

        {sections.map((section) => {
          const selected = section.options.find((option) => option.value === section.value);
          const label = acpCompositeSectionLabel(section, t('acp.thoughtLevel'), thoughtLevelCount, t);
          return (
            <CompositeConfigSection
              key={section.id}
              id={section.id}
              label={label}
              labelClassName="w-40"
              valueLabel={selected?.name ?? section.valueLabel ?? ''}
              triggerAttributes={{
                'data-acp-composite-section': section.category,
                'data-acp-composite-section-label': label,
              }}
              value={section.value || UNSPECIFIED_ACP_CONFIG_VALUE}
              onValueChange={(value) => changeConfigOption?.(
                section.id,
                value === UNSPECIFIED_ACP_CONFIG_VALUE ? null : value,
              )}
              unspecifiedLabel={(section.showUnspecified ?? true) ? t('acp.unspecifiedThoughtLevel') : null}
              items={section.options}
              openSection={openSection}
              onOpenChange={changeOpenSection}
              onItemSelect={handleConfigOptionSelect}
            />
          );
        })}
        {menuFooter}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

type CompositeConfigItem = {
  value: string;
  name: string;
  description?: string | null;
  available?: boolean;
};

type CompositeConfigSectionProps = {
  id: string;
  label: string;
  labelClassName: string;
  valueLabel: string;
  triggerAttributes?: Record<`data-${string}`, string>;
  value: string;
  onValueChange: (value: string) => void;
  /** null hides the unspecified option. */
  unspecifiedLabel: string | null;
  items: CompositeConfigItem[];
  openSection: AcpCompositeOpenSection | null;
  onOpenChange: (id: string, open: boolean, presentation?: AcpCompositeSectionPresentation) => void;
  onItemSelect: (event: Event) => void;
};

function CompositeConfigSection({
  id,
  label,
  labelClassName,
  valueLabel,
  triggerAttributes,
  value,
  onValueChange,
  unspecifiedLabel,
  items,
  openSection,
  onOpenChange,
  onItemSelect,
}: CompositeConfigSectionProps) {
  const triggerRef = useRef<HTMLDivElement>(null);
  const inlineId = useId();
  const { collisionBoundary, collisionPadding } = useOverlayPositioning();
  const presentation = openSection?.id === id ? openSection.presentation : null;

  // Measured on each open attempt so the decision follows the live pane width and menu position.
  const flyoutFits = () => {
    const menu = triggerRef.current?.closest('[data-slot="dropdown-menu-content"]');
    if (!menu) return true;
    const rem = Number.parseFloat(getComputedStyle(document.documentElement).fontSize) || 16;
    const width = Math.min(COMPOSITE_SUBMENU_WIDTH_REM * rem, window.innerWidth - 2 * rem);
    return overlayFitsBeside(
      menu.getBoundingClientRect(),
      overlayCollisionRect(collisionBoundary),
      width,
      COMPOSITE_SUBMENU_SIDE_OFFSET,
      collisionPadding,
    );
  };
  const handleFlyoutOpenChange = (open: boolean) => {
    if (open) {
      // Hover never expands inline; only an explicit click or key does.
      if (flyoutFits()) onOpenChange(id, true, 'flyout');
      return;
    }
    if (presentation === 'flyout') onOpenChange(id, false);
  };
  const handleClick = (event: MouseEvent<HTMLDivElement>) => {
    if (flyoutFits()) return;
    event.preventDefault();
    onOpenChange(id, presentation !== 'inline', 'inline');
  };
  const handleKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const expand = COMPOSITE_SUBMENU_OPEN_KEYS.includes(event.key);
    const collapse = event.key === 'ArrowLeft' && presentation === 'inline';
    if ((!expand && !collapse) || flyoutFits()) return;
    event.preventDefault();
    const toggles = event.key === 'Enter' || event.key === ' ';
    onOpenChange(id, collapse ? false : !(toggles && presentation === 'inline'), 'inline');
  };

  const radioGroup = (
    <DropdownMenuRadioGroup value={value} onValueChange={onValueChange}>
      {unspecifiedLabel !== null ? (
        <DropdownMenuRadioItem value={UNSPECIFIED_ACP_CONFIG_VALUE} onSelect={onItemSelect}>
          {unspecifiedLabel}
        </DropdownMenuRadioItem>
      ) : null}
      {items.map((item) => (
        <DropdownMenuRadioItem key={item.value} value={item.value} disabled={item.available === false} className="items-start py-2" onSelect={onItemSelect}>
          <span className="block min-w-0">
            <span className="block truncate font-medium">{item.name}</span>
            {item.description ? <span className="mt-0.5 block whitespace-normal break-words text-ui-caption leading-4 text-muted-foreground">{item.description}</span> : null}
          </span>
        </DropdownMenuRadioItem>
      ))}
    </DropdownMenuRadioGroup>
  );

  return (
    <>
      <DropdownMenuSub open={presentation === 'flyout'} onOpenChange={handleFlyoutOpenChange}>
        <DropdownMenuSubTrigger
          ref={triggerRef}
          className={cn('py-2', presentation === 'inline' && '[&>svg:last-child]:rotate-90')}
          {...triggerAttributes}
          {...(presentation === 'inline'
            ? { 'aria-expanded': true, 'aria-controls': inlineId, 'data-state': 'open' }
            : null)}
          onClick={handleClick}
          onKeyDown={handleKeyDown}
        >
          <span className={cn(labelClassName, 'shrink-0 truncate text-muted-foreground')}>{label}</span>
          <span className="min-w-0 flex-1 truncate text-right text-foreground">{valueLabel}</span>
        </DropdownMenuSubTrigger>
        <DropdownMenuSubContent sideOffset={COMPOSITE_SUBMENU_SIDE_OFFSET} className={COMPOSITE_SUBMENU_CLASS}>
          {radioGroup}
        </DropdownMenuSubContent>
      </DropdownMenuSub>
      {presentation === 'inline' ? (
        <div id={inlineId} role="group" aria-label={label} data-acp-composite-inline={id} className="pl-3">
          {radioGroup}
        </div>
      ) : null}
    </>
  );
}

function resolveCompositeSections({
  compositeSections,
  configOptions,
  configOptionValues,
  modelValue,
  thoughtLevel,
  thoughtValue,
  thoughtValueLabel,
  showUnspecifiedThought,
  modelBoundCatalogs,
}: {
  compositeSections?: AcpCompositeConfigSection[] | null;
  configOptions?: AcpSelectConfigOptionVm[] | null;
  configOptionValues?: Record<string, string> | null;
  modelValue?: string | null;
  thoughtLevel?: ThoughtLevelProps | null;
  thoughtValue?: string | null;
  thoughtValueLabel?: string | null;
  showUnspecifiedThought: boolean;
  modelBoundCatalogs?: Record<string, AcpSelectConfigOptionVm[]> | null;
}): AcpCompositeConfigSection[] {
  if (compositeSections) return compositeSections.filter((section) => section.options.length > 0);
  if (configOptions) {
    return acpCompositeConfigSections(
      configOptions,
      modelValue,
      configOptionValues,
      modelBoundCatalogs,
    );
  }
  if (!thoughtLevel || thoughtLevel.options.length === 0) return [];
  return [{
    id: thoughtLevel.id,
    category: thoughtLevel.category ?? ACP_THOUGHT_LEVEL_CATEGORY,
    name: thoughtLevel.name ?? null,
    description: thoughtLevel.description ?? null,
    currentValue: thoughtLevel.currentValue,
    value: thoughtValue ?? null,
    valueLabel: thoughtValueLabel,
    showUnspecified: showUnspecifiedThought,
    options: thoughtLevel.options,
  }];
}
