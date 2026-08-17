import { type ClassValue, clsx } from 'clsx';
import { extendTailwindMerge } from 'tailwind-merge';

const mergeTailwindClasses = extendTailwindMerge({
  extend: {
    classGroups: {
      'font-size': [
        'text-ui-nano',
        'text-ui-micro',
        'text-ui-caption',
        'text-ui-compact',
      ],
    },
  },
});

export function cn(...inputs: ClassValue[]) {
  return mergeTailwindClasses(clsx(inputs));
}
