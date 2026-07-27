import { Pencil } from 'lucide-react';
import { useCallback, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { cn } from '@/lib/utils';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';

interface EditableConversationTitleProps {
  title: string;
  metadata?: string | null;
  className?: string;
  showEditIcon?: boolean;
  onTitleChange?: (title: string) => void;
}

export function EditableConversationTitle({
  title,
  metadata,
  className,
  showEditIcon = true,
  onTitleChange,
}: EditableConversationTitleProps) {
  const { t } = useTranslation();
  const [editing, setEditing] = useState(false);
  const [value, setValue] = useState(title);
  const inputRef = useRef<HTMLInputElement>(null);

  const startEditing = useCallback(() => {
    setValue(title);
    setEditing(true);
    requestAnimationFrame(() => inputRef.current?.select());
  }, [title]);

  const commitTitle = useCallback(() => {
    setEditing(false);
    const trimmed = value.trim();
    if (trimmed && trimmed !== title) {
      onTitleChange?.(trimmed);
    }
  }, [onTitleChange, title, value]);

  const handleTitleKeyDown = useCallback((event: React.KeyboardEvent) => {
    if (event.key === 'Enter') {
      event.preventDefault();
      commitTitle();
    }
    if (event.key === 'Escape') {
      setValue(title);
      setEditing(false);
    }
  }, [commitTitle, title]);

  if (editing) {
    return (
      <input
        ref={inputRef}
        className={cn(
          'min-w-0 rounded-md border border-primary/40 bg-background px-2 py-0.5 text-sm font-semibold text-foreground outline-none ring-2 ring-primary/10',
          className,
        )}
        value={value}
        onChange={(event) => setValue(event.target.value)}
        onBlur={commitTitle}
        onKeyDown={handleTitleKeyDown}
      />
    );
  }

  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <button
          type="button"
          className={cn(
            'group -ml-1 flex min-w-0 items-center gap-1.5 rounded-md px-1 py-0.5 text-left transition-colors hover:bg-muted/50',
            className,
          )}
          onClick={startEditing}
        >
          <h1 className="min-w-0 truncate text-sm font-semibold leading-6 text-foreground">{title}</h1>
          {metadata ? <span className="shrink-0 text-[10px] text-muted-foreground/60">{metadata}</span> : null}
          {showEditIcon ? (
            <Pencil className="size-3 shrink-0 text-muted-foreground opacity-0 transition-opacity group-hover:opacity-100" />
          ) : null}
        </button>
      </TooltipTrigger>
      <TooltipContent>{t('conversation.runtime.titleEdit')}</TooltipContent>
    </Tooltip>
  );
}
