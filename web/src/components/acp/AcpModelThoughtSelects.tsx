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

export { UNSPECIFIED_ACP_CONFIG_VALUE } from '@/components/acp/AcpSingleConfigMenu';

export const ACP_THOUGHT_LEVEL_CATEGORY = 'thought_level';

export function findAcpThoughtLevel(
  configOptions: AcpSelectConfigOptionVm[] | null | undefined,
) {
  return configOptions?.find((option) => option.category === ACP_THOUGHT_LEVEL_CATEGORY) ?? null;
}

export function acpConfigMenuSelectionMode(
  thoughtLevel: AcpSelectConfigOptionVm | null | undefined,
) {
  return thoughtLevel && thoughtLevel.options.length > 0 ? 'composite' : 'single';
}

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

export function nextAcpCompositeSection(
  currentSection: string | null,
  section: string,
  open: boolean,
) {
  if (open) return section;
  return currentSection === section ? null : currentSection;
}

export function formatAcpCompositeSelection(
  modelName: string | null | undefined,
  thoughtName: string | null | undefined,
  unspecifiedLabel: string,
) {
  if (modelName && thoughtName) return `${modelName} · ${thoughtName}`;
  if (modelName) return modelName;
  if (thoughtName) return `${unspecifiedLabel} · ${thoughtName}`;
  return unspecifiedLabel;
}

type Props = {
  models: AcpModeVm[];
  modelValue?: string | null;
  thoughtLevel?: AcpSelectConfigOptionVm | null;
  thoughtValue?: string | null;
  onModelChange: (value: string | null) => void;
  onThoughtChange?: (optionId: string, value: string | null) => void;
  showUnspecifiedModel?: boolean;
  showUnspecifiedThought?: boolean;
  compact?: boolean;
  contentSide?: 'top' | 'bottom';
  align?: 'start' | 'end';
  triggerClassName?: string;
};

export function AcpModelThoughtSelects({
  models,
  modelValue,
  thoughtLevel,
  thoughtValue,
  onModelChange,
  onThoughtChange,
  showUnspecifiedModel = true,
  showUnspecifiedThought = true,
  compact = false,
  contentSide = 'bottom',
  align = DEFAULT_ACP_COMPOSER_CONFIG_ALIGN,
  triggerClassName,
}: Props) {
  const { t } = useTranslation();
  const [menuOpen, setMenuOpen] = useState(false);
  const [openSection, setOpenSection] = useState<string | null>(null);
  const keepMenuOpenRef = useRef(false);
  const triggerClass = acpComposerConfigTriggerVariants({ compact });
  const selectedModel = models.find((model) => model.id === modelValue);
  const selectedThought = thoughtLevel?.options.find((option) => option.value === thoughtValue);
  const selectionMode = acpConfigMenuSelectionMode(thoughtLevel);
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
        options={models}
        unspecifiedLabel={t('conversation.home.unspecifiedModel')}
        onValueChange={onModelChange}
        showUnspecified={showUnspecifiedModel}
        compact={compact}
        contentSide={contentSide}
        align={align}
        triggerClassName={triggerClassName}
      />
    ) : null;
  }

  const compositeLabel = formatAcpCompositeSelection(
    selectedModel?.name,
    selectedThought?.name,
    t('conversation.home.unspecifiedModel'),
  );

  return (
    <DropdownMenu
      open={menuOpen}
      modal={ACP_COMPOSER_CONFIG_DROPDOWN_MODAL}
      onOpenChange={handleMenuOpenChange}
    >
      <DropdownMenuTrigger className={cn(triggerClass, triggerClassName)}>
        <span className={ACP_COMPOSER_CONFIG_TRIGGER_LABEL_CLASS}>{t('acp.currentModel')}</span>
        <span className={ACP_COMPOSER_CONFIG_TRIGGER_VALUE_CLASS}>{compositeLabel}</span>
        <ChevronDown className={ACP_COMPOSER_CONFIG_TRIGGER_ICON_CLASS} />
      </DropdownMenuTrigger>
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
            <span className="w-20 shrink-0 text-muted-foreground">{t('acp.currentModel')}</span>
            <span className="min-w-0 flex-1 truncate text-right text-foreground">{selectedModel?.name ?? ''}</span>
          </DropdownMenuSubTrigger>
          <DropdownMenuSubContent
            sideOffset={6}
            className="w-[min(22rem,calc(100vw-2rem))] max-h-[min(24rem,var(--radix-dropdown-menu-content-available-height))] overflow-y-auto"
          >
            <DropdownMenuRadioGroup
              value={modelValue || UNSPECIFIED_ACP_CONFIG_VALUE}
              onValueChange={(value) => onModelChange(value === UNSPECIFIED_ACP_CONFIG_VALUE ? null : value)}
            >
              {showUnspecifiedModel ? (
                <DropdownMenuRadioItem value={UNSPECIFIED_ACP_CONFIG_VALUE} onSelect={handleConfigOptionSelect}>
                  {t('conversation.home.unspecifiedModel')}
                </DropdownMenuRadioItem>
              ) : null}
              {models.map((model) => (
                <DropdownMenuRadioItem key={model.id} value={model.id} className="items-start py-2" onSelect={handleConfigOptionSelect}>
                  <span className="block min-w-0">
                    <span className="block truncate font-medium">{model.name}</span>
                    {model.description ? <span className="mt-0.5 block whitespace-normal break-words text-ui-caption leading-4 text-muted-foreground">{model.description}</span> : null}
                  </span>
                </DropdownMenuRadioItem>
              ))}
            </DropdownMenuRadioGroup>
          </DropdownMenuSubContent>
        </DropdownMenuSub>

        <DropdownMenuSub
          open={openSection === thoughtLevel!.id}
          onOpenChange={(open) => setOpenSection((current) => nextAcpCompositeSection(current, thoughtLevel!.id, open))}
        >
          <DropdownMenuSubTrigger className="py-2">
            <span className="w-20 shrink-0 text-muted-foreground">{t('acp.thoughtLevel')}</span>
            <span className="min-w-0 flex-1 truncate text-right text-foreground">{selectedThought?.name ?? ''}</span>
          </DropdownMenuSubTrigger>
          <DropdownMenuSubContent
            sideOffset={6}
            className="w-[min(22rem,calc(100vw-2rem))] max-h-[min(24rem,var(--radix-dropdown-menu-content-available-height))] overflow-y-auto"
          >
            <DropdownMenuRadioGroup
              value={thoughtValue || UNSPECIFIED_ACP_CONFIG_VALUE}
              onValueChange={(value) => onThoughtChange?.(
                thoughtLevel!.id,
                value === UNSPECIFIED_ACP_CONFIG_VALUE ? null : value,
              )}
            >
              {showUnspecifiedThought ? (
                <DropdownMenuRadioItem value={UNSPECIFIED_ACP_CONFIG_VALUE} onSelect={handleConfigOptionSelect}>
                  {t('acp.unspecifiedThoughtLevel')}
                </DropdownMenuRadioItem>
              ) : null}
              {thoughtLevel!.options.map((option) => (
                <DropdownMenuRadioItem key={option.value} value={option.value} className="items-start py-2" onSelect={handleConfigOptionSelect}>
                  <span className="block min-w-0">
                    <span className="block truncate font-medium">{option.name}</span>
                    {option.description ? <span className="mt-0.5 block whitespace-normal break-words text-ui-caption leading-4 text-muted-foreground">{option.description}</span> : null}
                  </span>
                </DropdownMenuRadioItem>
              ))}
            </DropdownMenuRadioGroup>
          </DropdownMenuSubContent>
        </DropdownMenuSub>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
