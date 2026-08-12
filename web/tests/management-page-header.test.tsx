import React from 'react';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';
import { PageContent, PageHeader, pageContentStyles, pageHeaderStyles } from '@/components/PageScaffold';

const managementPageSources = [
  '../src/pages/AgentManagementPage.tsx',
  '../src/pages/ContextManagementPage.tsx',
  '../src/pages/RunModeManagementPage.tsx',
  '../src/pages/ScheduledTaskManagementPage.tsx',
].map((path) => readFileSync(fileURLToPath(new URL(path, import.meta.url)), 'utf8'));

describe('integrated management page header', () => {
  it('uses the page surface without a divider, overlay, or oversized title', () => {
    expect(pageHeaderStyles.integrated.root).not.toContain('border-b');
    expect(pageHeaderStyles.integrated.root).not.toContain('bg-');
    expect(pageHeaderStyles.integrated.root).not.toContain('backdrop-blur');
    expect(pageHeaderStyles.integrated.title).toBe('text-lg');
    expect(pageHeaderStyles.integrated.headingRow).toContain('sm:items-start');
    expect(pageHeaderStyles.integrated.headingRow).not.toContain('sm:items-center');

    const html = renderToStaticMarkup(
      <PageHeader
        variant="integrated"
        icon={<svg data-testid="test-icon" />}
        title="管理页"
        actions={<button type="button">刷新</button>}
        navigation={<span>一级导航</span>}
        navigationLabel="管理页分区"
      />,
    );

    expect(html).toContain('data-variant="integrated"');
    expect(html).toContain('data-slot="page-header-identity" class="flex min-w-0 items-center gap-3"');
    expect(html).toContain('data-slot="page-header-icon"');
    expect(html).toContain('aria-hidden="true"');
    expect(html).toContain('text-foreground [&amp;_svg]:size-5');
    expect(html).not.toContain('page-header-icon" aria-hidden="true" class="shrink-0 text-primary');
    expect(html).toContain('<h1 class="min-w-0 truncate font-semibold tracking-tight text-foreground text-lg">');
    expect(html).toContain('<nav class="min-w-0" aria-label="管理页分区">');
    expect(pageHeaderStyles.integrated.root).toContain('px-6');
    expect(pageHeaderStyles.integrated.root).toContain('pt-8');
    expect(pageHeaderStyles.integrated.root).not.toContain('px-5');
    expect(pageHeaderStyles.integrated.root).not.toContain('xl:px-6');
    expect(pageHeaderStyles.integrated.root).not.toContain('xl:pt-6');
    expect(pageHeaderStyles.integrated.navigationRoot).toBe('pb-1');
  });

  it('uses a compact shared content inset after page navigation', () => {
    const html = renderToStaticMarkup(<PageContent variant="after-navigation">内容</PageContent>);

    expect(pageContentStyles['after-navigation']).toContain('pt-2');
    expect(pageContentStyles['after-navigation']).not.toContain('pt-4');
    expect(html).toContain('data-variant="after-navigation"');
  });

  it('is consumed by Agent, context, run-mode, and scheduled-task management pages', () => {
    for (const source of managementPageSources) {
      expect(source).toContain('variant="integrated"');
    }
    expect(managementPageSources[0]).toContain('icon={<Bot />}');
    expect(managementPageSources[1]).toContain('icon={<Library />}');
    expect(managementPageSources[2]).toContain('icon={<Route />}');
    expect(managementPageSources[3]).toContain('icon={<AlarmClock />}');
    expect(managementPageSources[3]).toContain('badges={<span');
    expect(managementPageSources[1]).toContain('navigation={(');
    expect(managementPageSources[1]).toContain("navigationLabel={t('contextManagement.title')}");
    expect(managementPageSources[1]).toContain('<TabsList variant="line"');
    expect(managementPageSources[1]).not.toContain('<div className="border-b px-5 xl:px-6">');
  });
});
