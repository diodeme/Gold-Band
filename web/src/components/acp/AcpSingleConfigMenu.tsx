import { ChevronDown } from 'lucide-react';

import {
  ACP_COMPOSER_CONFIG_DROPDOWN_MODAL,
  ACP_COMPOSER_CONFIG_TRIGGER_ICON_CLASS,
  ACP_COMPOSER_CONFIG_TRIGGER_LABEL_CLASS,
  ACP_COMPOSER_CONFIG_TRIGGER_VALUE_CLASS,
  DEFAULT_ACP_COMPOSER_CONFIG_ALIGN,
  acpComposerConfigTriggerVariants,
} from '@/components/acp/AcpComposerConfigTrigger';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';

export const UNSPECIFIED_ACP_CONFIG_VALUE = '__gold_band_unspecified__';

export type AcpSingleConfigMenuOption = {
  id: string;
  name: string;
  description?: string | null;
};

type Props = {
  label: string;
  value?: string | null;
  valueLabel?: string | null;
  options: AcpSingleConfigMenuOption[];
  unspecifiedLabel: string;
  onValueChange: (value: string | null) => void;
  showUnspecified?: boolean;
  compact?: boolean;
  contentSide?: 'top' | 'bottom';
  align?: 'start' | 'end';
};

export function resolveAcpSingleConfigMenuValue(value: string) {
  return value === UNSPECIFIED_ACP_CONFIG_VALUE ? null : value;
}

export function AcpSingleConfigMenu({
  label,
  value,
  valueLabel,
  options,
  unspecifiedLabel,
  onValueChange,
  showUnspecified = true,
  compact = false,
  contentSide = 'bottom',
  align = DEFAULT_ACP_COMPOSER_CONFIG_ALIGN,
}: Props) {
  const selectedOption = options.find((option) => option.id === value);
  const selectedLabel = valueLabel ?? selectedOption?.name ?? unspecifiedLabel;

  return (
    <DropdownMenu modal={ACP_COMPOSER_CONFIG_DROPDOWN_MODAL}>
      <DropdownMenuTrigger className={acpComposerConfigTriggerVariants({ compact })}>
        <span className={ACP_COMPOSER_CONFIG_TRIGGER_LABEL_CLASS}>{label}</span>
        <span className={ACP_COMPOSER_CONFIG_TRIGGER_VALUE_CLASS}>{selectedLabel}</span>
        <ChevronDown className={ACP_COMPOSER_CONFIG_TRIGGER_ICON_CLASS} />
      </DropdownMenuTrigger>
      <DropdownMenuContent
        side={contentSide}
        sideOffset={8}
        align={align}
        className="w-[min(22rem,calc(100vw-2rem))] max-w-[calc(100vw-2rem)]"
      >
        <DropdownMenuRadioGroup
          value={value || UNSPECIFIED_ACP_CONFIG_VALUE}
          onValueChange={(nextValue) => onValueChange(resolveAcpSingleConfigMenuValue(nextValue))}
        >
          {showUnspecified ? (
            <DropdownMenuRadioItem value={UNSPECIFIED_ACP_CONFIG_VALUE}>
              {unspecifiedLabel}
            </DropdownMenuRadioItem>
          ) : null}
          {options.map((option) => (
            <DropdownMenuRadioItem key={option.id} value={option.id} className="items-start py-2">
              <span className="block min-w-0">
                <span className="block truncate font-medium">{option.name}</span>
                {option.description ? (
                  <span className="mt-0.5 block whitespace-normal break-words text-[11px] leading-4 text-muted-foreground">
                    {option.description}
                  </span>
                ) : null}
              </span>
            </DropdownMenuRadioItem>
          ))}
        </DropdownMenuRadioGroup>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
