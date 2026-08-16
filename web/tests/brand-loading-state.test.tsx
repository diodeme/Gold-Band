import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';

import { BrandLoadingState } from '@/components/BrandLoadingState';

describe('BrandLoadingState', () => {
  it('uses the canonical app logo and exposes an accessible loading status', () => {
    const html = renderToStaticMarkup(<BrandLoadingState label="正在加载会话" />);

    expect(html).toContain('data-brand-loading-state="true"');
    expect(html).toContain('role="status"');
    expect(html).toContain('aria-label="正在加载会话"');
    expect(html).toContain('src="/logo.svg"');
    expect(html).toContain('brand-loading-logo');
  });
});
