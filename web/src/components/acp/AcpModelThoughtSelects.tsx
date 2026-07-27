import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { ChevronDown } from 'lucide-react';

import type { AcpModeVm, AcpSelectConfigOptionVm } from '@/types';
import {
  ACP_COMPOSER_CONFIG_TRIGGER_ICON_CLASS,
  ACP_COMPOSER_CONFIG_TRIGGER_LABEL_CLASS,
  ACP_COMPOSER_CONFIG_TRIGGER_VALUE_CLASS,
  ACP_COMPOSER_CONFIG_DROPDOWN_MODAL,
  DEFAULT_ACP_COMPOSER_CONFIG_ALIGN,
  acpComposerConfigTriggerVariants,
} from '@/components/acp/AcpComposerConfigTrigger';
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
import { Select, SelectContent, SelectItem, SelectTrigger } from '@/components/ui/select';

export const UNSPECIFIED_ACP_CONFIG_VALUE = '__gold_band_unspecified__';

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
  compact?: boolean;
  contentSide?: 'top' | 'bottom';
  align?: 'start' | 'end';
};

export function AcpModelThoughtSelects({
  models,
  modelValue,
  thoughtLevel,
  thoughtValue,
  onModelChange,
  onThoughtChange,
  compact = false,
  contentSide = 'bottom',
  align = DEFAULT_ACP_COMPOSER_CONFIG_ALIGN,
}: Props) {
  const { t } = useTranslation();
  const [openSection, setOpenSection] = useState<string | null>(null);
  const triggerClass = acpComposerConfigTriggerVariants({ compact });
  const selectedModel = models.find((model) => model.id === modelValue);
  const selectedThought = thoughtLevel?.options.find((option) => option.value === thoughtValue);
  const hasThoughtLevel = Boolean(thoughtLevel && thoughtLevel.options.length > 0);

  if (!hasThoughtLevel) {
    return models.length > 0 ? (
      <Select
        value={modelValue || UNSPECIFIED_ACP_CONFIG_VALUE}
        onValueChange={(value) => onModelChange(value === UNSPECIFIED_ACP_CONFIG_VALUE ? null : value)}
      >
        <SelectTrigger className={triggerClass}>
          <span className={ACP_COMPOSER_CONFIG_TRIGGER_LABEL_CLASS}>{t('acp.currentModel')}</span>
          <span className={ACP_COMPOSER_CONFIG_TRIGGER_VALUE_CLASS}>
            {selectedModel?.name ?? t('conversation.home.unspecifiedModel')}
          </span>
        </SelectTrigger>
        <SelectContent
          side={contentSide}
          sideOffset={8}
          position="popper"
          align={align}
          className="w-[min(24rem,calc(100vw-2rem))]"
        >
          <SelectItem value={UNSPECIFIED_ACP_CONFIG_VALUE}>{t('conversation.home.unspecifiedModel')}</SelectItem>
          {models.map((model) => (
            <SelectItem key={model.id} value={model.id} className="items-start py-2">
              <span className="block min-w-0">
                <span className="block truncate font-medium">{model.name}</span>
                {model.description ? <span className="mt-0.5 block whitespace-normal break-words text-[11px] leading-4 text-muted-foreground">{model.description}</span> : null}
              </span>
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    ) : null;
  }

  const compositeLabel = formatAcpCompositeSelection(
    selectedModel?.name,
    selectedThought?.name,
    t('conversation.home.unspecifiedModel'),
  );

  return (
    <DropdownMenu
      modal={ACP_COMPOSER_CONFIG_DROPDOWN_MODAL}
      onOpenChange={() => setOpenSection(null)}
    >
      <DropdownMenuTrigger className={triggerClass}>
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
          <DropdownMenuSubTrigger
            className="py-2"
            onClick={() => setOpenSection((current) => nextAcpCompositeSection(current, 'model', current !== 'model'))}
          >
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
              <DropdownMenuRadioItem value={UNSPECIFIED_ACP_CONFIG_VALUE}>
                {t('conversation.home.unspecifiedModel')}
              </DropdownMenuRadioItem>
              {models.map((model) => (
                <DropdownMenuRadioItem key={model.id} value={model.id} className="items-start py-2">
                  <span className="block min-w-0">
                    <span className="block truncate font-medium">{model.name}</span>
                    {model.description ? <span className="mt-0.5 block whitespace-normal break-words text-[11px] leading-4 text-muted-foreground">{model.description}</span> : null}
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
          <DropdownMenuSubTrigger
            className="py-2"
            onClick={() => setOpenSection((current) => nextAcpCompositeSection(current, thoughtLevel!.id, current !== thoughtLevel!.id))}
          >
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
              <DropdownMenuRadioItem value={UNSPECIFIED_ACP_CONFIG_VALUE}>
                {t('acp.unspecifiedThoughtLevel')}
              </DropdownMenuRadioItem>
              {thoughtLevel!.options.map((option) => (
                <DropdownMenuRadioItem key={option.value} value={option.value} className="items-start py-2">
                  <span className="block min-w-0">
                    <span className="block truncate font-medium">{option.name}</span>
                    {option.description ? <span className="mt-0.5 block whitespace-normal break-words text-[11px] leading-4 text-muted-foreground">{option.description}</span> : null}
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
