import { Bot, User } from 'lucide-react';
import type { AvatarKind, AvatarProfileVm } from '@/types';
import { Avatar, AvatarFallback, AvatarImage } from '@/components/ui/avatar';
import { avatarShapeClass, selectedAvatar } from '@/lib/avatar';
import { cn } from '@/lib/utils';

interface AvatarDisplayProps {
  kind: AvatarKind;
  profile: AvatarProfileVm;
  className?: string;
  fallbackClassName?: string;
}

export function AvatarDisplay({ kind, profile, className, fallbackClassName }: AvatarDisplayProps) {
  const image = selectedAvatar(profile);
  const Icon = kind === 'agent' ? Bot : User;
  const shapeClass = avatarShapeClass(profile.shape);
  return (
    <Avatar className={cn('size-10 border border-border/60 bg-muted/30', shapeClass, className)}>
      {image ? <AvatarImage src={image.dataUrl} alt="" className={cn('object-cover', shapeClass)} /> : null}
      <AvatarFallback className={cn('bg-muted/45 text-muted-foreground', shapeClass, fallbackClassName)}>
        <Icon className="size-[45%]" aria-hidden="true" />
      </AvatarFallback>
    </Avatar>
  );
}
