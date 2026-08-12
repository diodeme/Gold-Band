import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';
import { AppProviders } from '@/components/AppProviders';
import { Tooltip, TooltipTrigger } from '@/components/ui/tooltip';

describe('AppProviders', () => {
  it('provides tooltip context for pages that render execution-history actions', () => {
    expect(() => renderToStaticMarkup(
      React.createElement(
        AppProviders,
        null,
        React.createElement(
          Tooltip,
          null,
          React.createElement(TooltipTrigger, null, 'Open run'),
        ),
      ),
    )).not.toThrow();
  });
});
