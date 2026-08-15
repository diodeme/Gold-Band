import { Badge } from '@/components/ui/badge';
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui/tooltip';

interface SlashCommandInputTagProps {
  prefix: string;
  description?: string;
}

export function SlashCommandInputTag({ prefix, description }: SlashCommandInputTagProps) {
  const tag = (
    <Badge
      variant="secondary"
      data-slot="slash-command-input-tag"
      className="shrink-0 rounded-md border border-border/70 bg-secondary/85 px-2 py-1 text-xs font-medium leading-4 text-secondary-foreground shadow-xs"
    >
      {prefix}
    </Badge>
  );

  if (!description) return tag;

  return (
    <TooltipProvider delayDuration={300}>
      <Tooltip>
        <TooltipTrigger asChild>{tag}</TooltipTrigger>
        <TooltipContent side="top" sideOffset={6} className="max-w-80 text-pretty leading-5">
          {description}
        </TooltipContent>
      </Tooltip>
    </TooltipProvider>
  );
}
