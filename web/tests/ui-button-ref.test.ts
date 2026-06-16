import { describe, expect, it } from 'vitest';
import { Button } from '@/components/ui/button';
import { AlertDialogTrigger } from '@/components/ui/alert-dialog';
import { CollapsibleTrigger } from '@/components/ui/collapsible';
import { DialogClose, DialogTrigger } from '@/components/ui/dialog';
import { DropdownMenuTrigger } from '@/components/ui/dropdown-menu';
import { PopoverTrigger } from '@/components/ui/popover';
import { SelectTrigger } from '@/components/ui/select';
import { SheetClose, SheetTrigger } from '@/components/ui/sheet';
import { TooltipTrigger } from '@/components/ui/tooltip';

function expectForwardRef(component: unknown) {
  const marker = (component as { $$typeof?: symbol }).$$typeof;

  expect(marker?.description).toBe('react.forward_ref');
}

describe('ui Button', () => {
  it('forwards refs for Radix asChild trigger composition', () => {
    expectForwardRef(Button);
  });

  it('keeps Radix trigger wrappers ref-forwarding', () => {
    [
      AlertDialogTrigger,
      CollapsibleTrigger,
      DialogClose,
      DialogTrigger,
      DropdownMenuTrigger,
      PopoverTrigger,
      SelectTrigger,
      SheetClose,
      SheetTrigger,
      TooltipTrigger,
    ].forEach(expectForwardRef);
  });
});
