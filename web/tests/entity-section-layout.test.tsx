import React from 'react';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';
import { EntitySection } from '@/components/EntitySection';

const contextSource = readFileSync(fileURLToPath(new URL('../src/pages/ContextManagementPage.tsx', import.meta.url)), 'utf8');

describe('context management entity section layout', () => {
  it('uses one shared line-tab header contract for arbitrary entity domains', () => {
    const html = renderToStaticMarkup(
      <EntitySection
        tab="global"
        onTabChange={() => undefined}
        tabs={[
          { value: 'global', label: '全局 SKILL' },
          { value: 'project', label: '项目 SKILL' },
        ]}
        actions={<button type="button">刷新</button>}
      >
        内容
      </EntitySection>,
    );

    expect(html).toContain('data-slot="card-header"');
    expect(html).toContain('border-b px-4 pt-2 [.border-b]:pb-1');
    expect(html).toContain('data-slot="tabs-list" data-variant="line"');
    expect(html).toContain('全局 SKILL');
    expect(html).toContain('项目 SKILL');
  });

  it('routes profile, MCP, and SKILL inner tabs through EntitySection', () => {
    expect(contextSource.match(/<EntitySection/g)).toHaveLength(3);
    expect(contextSource.match(/<PageContent variant="after-navigation">/g)).toHaveLength(3);
    expect(contextSource).not.toContain('<AppCard className="flex h-full min-h-0 flex-col gap-0 py-0">');
    expect(contextSource).toContain("{ value: 'global', label: t('contextManagement.skills.globalTab'");
  });

  it('uses one tooltip-backed icon refresh button without native title tooltips', () => {
    expect(contextSource).toContain('function EntityRefreshButton');
    expect(contextSource).toContain('<TooltipTrigger asChild>');
    expect(contextSource).toContain('<TooltipContent>{label}</TooltipContent>');
    expect(contextSource.match(/<EntityRefreshButton/g)).toHaveLength(3);
    expect(contextSource).not.toContain("title={t('common.refresh')}");
  });
});
