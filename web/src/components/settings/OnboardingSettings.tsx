import { useId } from 'react';
import { useTranslation } from 'react-i18next';
import { Button } from '@/components/ui/button';

const setupActions = [
  {
    id: 'setup',
    titleKey: 'onboarding.rerunTitle',
    descriptionKey: 'onboarding.rerunDescription',
    actionKey: 'onboarding.rerun',
    route: '/onboarding',
  },
  {
    id: 'defaults',
    titleKey: 'onboarding.defaultsTitle',
    descriptionKey: 'onboarding.defaultsDescription',
    actionKey: 'onboarding.configureDefaults',
    route: '/onboarding/defaults',
  },
] as const;

export function OnboardingSettings() {
  const { t } = useTranslation();
  const descriptionId = useId();
  return (
    <div className="divide-y divide-border/40">
      {setupActions.map((action) => (
        <div key={action.id} className="flex min-w-0 flex-wrap items-center justify-between gap-x-4 gap-y-3 py-3 first:pt-0 last:pb-0">
          <div className="min-w-0 flex-1 basis-64 space-y-1">
            <h3 className="text-sm font-medium text-foreground">{t(action.titleKey)}</h3>
            <p id={`${descriptionId}-${action.id}`} className="text-xs leading-5 text-muted-foreground">{t(action.descriptionKey)}</p>
          </div>
          <Button
            type="button"
            variant="outline"
            size="sm"
            className="h-auto min-h-8 max-w-full whitespace-normal"
            aria-describedby={`${descriptionId}-${action.id}`}
            onClick={() => { window.location.hash = action.route; }}
          >
            {t(action.actionKey)}
          </Button>
        </div>
      ))}
    </div>
  );
}
