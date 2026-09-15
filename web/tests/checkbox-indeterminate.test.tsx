/** @vitest-environment jsdom */

import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, describe, expect, it } from 'vitest';

import { Checkbox } from '@/components/ui/checkbox';

// 三态 Checkbox 是 MulticaSkillSyncDialog 全选开关的依赖：indeterminate 必须渲染
// 减号图标并带主题 accent 填充（ui-interaction.md §8 选中态用主题色），
// 否则部分勾选态视觉上与未勾选混淆（Radix Indicator 对 indeterminate 也渲染，
// copy-in 原版只有 CheckIcon + checked 态样式时会出现「无填充方框里的对勾」）。

function renderChecked(checked: boolean | 'indeterminate') {
  const container = document.createElement('div');
  document.body.appendChild(container);
  const root = createRoot(container);
  act(() => {
    root.render(<Checkbox checked={checked} onCheckedChange={() => {}} />);
  });
  return container;
}

afterEach(() => {
  document.body.innerHTML = '';
});

describe('shared checkbox indeterminate support', () => {
  it('renders a minus icon with theme accent for indeterminate state', () => {
    const container = renderChecked('indeterminate');
    const root = container.querySelector('[data-slot="checkbox"]');
    expect(root?.getAttribute('data-state')).toBe('indeterminate');
    // 主题 accent 填充契约（jsdom 无 tailwind 运行时，固化到 class 投影）。
    expect(root?.className).toContain('data-[state=indeterminate]:bg-primary');
    expect(container.querySelector('svg.lucide-minus')).toBeTruthy();
    expect(container.querySelector('svg.lucide-check')).toBeNull();
  });

  it('renders a check icon for fully checked state', () => {
    const container = renderChecked(true);
    expect(container.querySelector('[data-slot="checkbox"]')?.getAttribute('data-state')).toBe('checked');
    expect(container.querySelector('svg.lucide-check')).toBeTruthy();
    expect(container.querySelector('svg.lucide-minus')).toBeNull();
  });

  it('renders no indicator for unchecked state', () => {
    const container = renderChecked(false);
    expect(container.querySelector('svg.lucide-check')).toBeNull();
    expect(container.querySelector('svg.lucide-minus')).toBeNull();
  });
});
