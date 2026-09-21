import { ChevronDown } from 'lucide-react';

import { cn } from '@/lib/utils';

import {
  ACP_COMPOSER_CONFIG_DROPDOWN_MODAL,
  ACP_COMPOSER_CONFIG_TRIGGER_ICON_CLASS,
  ACP_COMPOSER_CONFIG_TRIGGER_LABEL_CLASS,
  ACP_COMPOSER_CONFIG_TRIGGER_VALUE_CLASS,
  DEFAULT_ACP_COMPOSER_CONFIG_ALIGN,
  acpComposerConfigTriggerVariants,
  formatAcpCompositeSelection,
  useAcpComposerConfigOverflowTooltip,
} from '@/components/acp/AcpComposerConfigTrigger';
import i18n from '@/i18n';
import { channelAppName } from '@/lib/channel-app-name';
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuLabel,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';

export const UNSPECIFIED_ACP_CONFIG_VALUE = '__gold_band_unspecified__';
export const ACP_AUTO_ACCEPT_GROUP_LABEL_CLASS =
  'pl-8 pr-2 py-1 text-ui-caption font-normal text-muted-foreground';

export type AcpSingleConfigMenuOption = {
  id: string;
  name: string;
  description?: string | null;
  available?: boolean;
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
  triggerClassName?: string;
  disabled?: boolean;
  autoAccept?: boolean;
  onAutoAcceptChange?: (enabled: boolean) => void;
  autoAcceptLabel?: string;
  autoAcceptGroupLabel?: string;
  appName?: string;
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
  triggerClassName,
  disabled = false,
  autoAccept = false,
  onAutoAcceptChange,
  autoAcceptLabel,
  autoAcceptGroupLabel,
  appName,
}: Props) {
  const selectedOption = options.find((option) => option.id === value);
  const selectedLabel = formatAcpCompositeSelection(
    valueLabel ?? selectedOption?.name,
    onAutoAcceptChange && autoAccept ? (autoAcceptLabel ?? i18n.t('acp.autoAccept')) : null,
    unspecifiedLabel,
  );
  const {
    valueRef,
    tooltipOpen,
    showTooltipIfOverflowing,
    hideTooltip,
    handleTooltipOpenChange,
  } = useAcpComposerConfigOverflowTooltip();

  return (
    <DropdownMenu modal={ACP_COMPOSER_CONFIG_DROPDOWN_MODAL}>
      <Tooltip open={tooltipOpen} onOpenChange={handleTooltipOpenChange}>
        <TooltipTrigger asChild>
          <DropdownMenuTrigger
            disabled={disabled}
            className={cn(acpComposerConfigTriggerVariants({ compact }), triggerClassName)}
            data-acp-auto-accept-overlay={onAutoAcceptChange ? 'true' : undefined}
            onPointerEnter={showTooltipIfOverflowing}
            onPointerLeave={hideTooltip}
            onPointerDown={hideTooltip}
            onFocus={showTooltipIfOverflowing}
            onBlur={hideTooltip}
          >
            <span className={ACP_COMPOSER_CONFIG_TRIGGER_LABEL_CLASS}>{label}</span>
            <span
              ref={valueRef}
              className={ACP_COMPOSER_CONFIG_TRIGGER_VALUE_CLASS}
              data-acp-config-value="true"
            >
              {selectedLabel}
            </span>
            <ChevronDown className={ACP_COMPOSER_CONFIG_TRIGGER_ICON_CLASS} />
          </DropdownMenuTrigger>
        </TooltipTrigger>
        <TooltipContent
          side="top"
          sideOffset={6}
          className="max-w-[min(24rem,calc(100vw-2rem))] whitespace-normal break-words"
        >
          {selectedLabel}
        </TooltipContent>
      </Tooltip>
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
            <DropdownMenuRadioItem
              key={option.id}
              value={option.id}
              disabled={option.available === false}
              className="items-start py-2"
            >
              <span className="block min-w-0">
                <span className="block truncate font-medium">{option.name}</span>
                {option.description ? (
                  <span className="mt-0.5 block whitespace-normal break-words text-ui-caption leading-4 text-muted-foreground">
                    {option.description}
                  </span>
                ) : null}
              </span>
            </DropdownMenuRadioItem>
          ))}
        </DropdownMenuRadioGroup>
        {onAutoAcceptChange ? (
          <>
            <DropdownMenuSeparator />
            <DropdownMenuLabel className={ACP_AUTO_ACCEPT_GROUP_LABEL_CLASS}>
              {autoAcceptGroupLabel ?? i18n.t('acp.autoAcceptGroup', { appName: appName ?? channelAppName() })}
            </DropdownMenuLabel>
            <DropdownMenuCheckboxItem
              checked={autoAccept}
              onCheckedChange={(checked) => onAutoAcceptChange(checked === true)}
              onSelect={(event) => event.preventDefault()}
            >
              {autoAcceptLabel ?? 'Auto Accept'}
            </DropdownMenuCheckboxItem>
          </>
        ) : null}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
