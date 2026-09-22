import { useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { ChevronDown } from 'lucide-react';

import type { AcpModeVm, AcpSelectConfigOptionVm } from '@/types';
import { cn } from '@/lib/utils';
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

export function nextAcpCompositeSection(
  currentSection: string | null,
  section: string,
  open: boolean,
) {
  if (open) return section;
  return currentSection === section ? null : currentSection;
}

type ThoughtLevelProps = Omit<AcpSelectConfigOptionVm, "options"> & {
  options: Array<AcpSelectConfigOptionVm["options"][number] & { available?: boolean }>;
};

type Props = {
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

export function AcpModelThoughtSelects({
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
}: Props) {
  const { t } = useTranslation();
  const [menuOpen, setMenuOpen] = useState(false);
  const [openSection, setOpenSection] = useState<string | null>(null);
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
    showTooltipIfOverflowing,
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
            onPointerEnter={showTooltipIfOverflowing}
            onPointerLeave={hideTooltip}
            onPointerDown={hideTooltip}
            onFocus={showTooltipIfOverflowing}
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
        <DropdownMenuSub
          open={openSection === 'model'}
          onOpenChange={(open) => setOpenSection((current) => nextAcpCompositeSection(current, 'model', open))}
        >
          <DropdownMenuSubTrigger className="py-2">
            <span className="w-20 shrink-0 truncate text-muted-foreground">{t('acp.currentModel')}</span>
            <span className="min-w-0 flex-1 truncate text-right text-foreground">{selectedModel?.name ?? modelValueLabel ?? ''}</span>
          </DropdownMenuSubTrigger>
          <DropdownMenuSubContent
            sideOffset={6}
            className="w-[min(22rem,calc(100vw-2rem))] max-h-[min(24rem,var(--radix-dropdown-menu-content-available-height))] overflow-y-auto"
          >
            <DropdownMenuRadioGroup
              value={modelValue || UNSPECIFIED_ACP_CONFIG_VALUE}
              onValueChange={(value) => {
                onModelChange(value === UNSPECIFIED_ACP_CONFIG_VALUE ? null : value);
              }}
            >
              {showUnspecifiedModel ? (
                <DropdownMenuRadioItem value={UNSPECIFIED_ACP_CONFIG_VALUE} onSelect={handleConfigOptionSelect}>
                  {unspecifiedLabel}
                </DropdownMenuRadioItem>
              ) : null}
              {models.map((model) => (
                <DropdownMenuRadioItem key={model.id} value={model.id} disabled={model.available === false} className="items-start py-2" onSelect={handleConfigOptionSelect}>
                  <span className="block min-w-0">
                    <span className="block truncate font-medium">{model.name}</span>
                    {model.description ? <span className="mt-0.5 block whitespace-normal break-words text-ui-caption leading-4 text-muted-foreground">{model.description}</span> : null}
                  </span>
                </DropdownMenuRadioItem>
              ))}
            </DropdownMenuRadioGroup>
          </DropdownMenuSubContent>
        </DropdownMenuSub>

        {sections.map((section) => {
          const selected = section.options.find((option) => option.value === section.value);
          const label = acpCompositeSectionLabel(section, t('acp.thoughtLevel'), thoughtLevelCount, t);
          const showUnspecified = section.showUnspecified ?? true;
          return (
            <DropdownMenuSub
              key={section.id}
              open={openSection === section.id}
              onOpenChange={(open) => setOpenSection((current) => nextAcpCompositeSection(current, section.id, open))}
            >
              <DropdownMenuSubTrigger
                className="py-2"
                data-acp-composite-section={section.category}
                data-acp-composite-section-label={label}
              >
                <span className="w-40 shrink-0 truncate text-muted-foreground">{label}</span>
                <span className="min-w-0 flex-1 truncate text-right text-foreground">{selected?.name ?? section.valueLabel ?? ''}</span>
              </DropdownMenuSubTrigger>
              <DropdownMenuSubContent
                sideOffset={6}
                className="w-[min(22rem,calc(100vw-2rem))] max-h-[min(24rem,var(--radix-dropdown-menu-content-available-height))] overflow-y-auto"
              >
                <DropdownMenuRadioGroup
                  value={section.value || UNSPECIFIED_ACP_CONFIG_VALUE}
                  onValueChange={(value) => changeConfigOption?.(
                    section.id,
                    value === UNSPECIFIED_ACP_CONFIG_VALUE ? null : value,
                  )}
                >
                  {showUnspecified ? (
                    <DropdownMenuRadioItem value={UNSPECIFIED_ACP_CONFIG_VALUE} onSelect={handleConfigOptionSelect}>
                      {t('acp.unspecifiedThoughtLevel')}
                    </DropdownMenuRadioItem>
                  ) : null}
                  {section.options.map((option) => (
                    <DropdownMenuRadioItem key={option.value} value={option.value} disabled={option.available === false} className="items-start py-2" onSelect={handleConfigOptionSelect}>
                      <span className="block min-w-0">
                        <span className="block truncate font-medium">{option.name}</span>
                        {option.description ? <span className="mt-0.5 block whitespace-normal break-words text-ui-caption leading-4 text-muted-foreground">{option.description}</span> : null}
                      </span>
                    </DropdownMenuRadioItem>
                  ))}
                </DropdownMenuRadioGroup>
              </DropdownMenuSubContent>
            </DropdownMenuSub>
          );
        })}
      </DropdownMenuContent>
    </DropdownMenu>
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
