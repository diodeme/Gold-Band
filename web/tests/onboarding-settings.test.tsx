/** @vitest-environment jsdom */

import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import i18n from '@/i18n';
import { OnboardingSettings } from '@/components/settings/OnboardingSettings';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

let root: Root | undefined;

beforeEach(async () => {
  window.location.hash = '#/settings';
  await i18n.changeLanguage('zh-CN');
});

afterEach(async () => {
  if (root) await act(async () => root?.unmount());
  root = undefined;
  document.body.replaceChildren();
});

async function renderSettings() {
  const container = document.createElement('div');
  document.body.append(container);
  root = createRoot(container);
  await act(async () => root?.render(<OnboardingSettings />));
  return container;
}

const settingsSource = readFileSync(
  resolve(__dirname, '../src/pages/SettingsPage.tsx'),
  'utf8',
);

describe('onboarding settings', () => {
  it('uses the same card and section hierarchy as the other general settings', () => {
    const general = settingsSource.match(/<TabsContent value="general"[\s\S]*?<\/TabsContent>/u)?.[0] ?? '';
    const title = general.indexOf("t('onboarding.settingsTitle')");
    expect(title).toBeGreaterThan(general.indexOf('<AppCard'));
    expect(title).toBeLessThan(general.indexOf('</AppCard>'));
    expect(general).toMatch(/<SettingsSection title=\{t\('onboarding.settingsTitle'\)\} divided>\s*<OnboardingSettings\s*\/>\s*<\/SettingsSection>/u);
  });

  it('separates both actions into described, wrapping rows with shared compact controls', async () => {
    const container = await renderSettings();
    expect([...container.querySelectorAll('h3')].map((heading) => heading.textContent))
      .toEqual(['首次启动引导', '默认工作流']);
    expect(container.textContent).toContain('重新检查 Git、运行环境和本机 Agent。');
    expect(container.textContent).toContain('统一设置内置工作流的 Agent、模型和权限，不影响自定义工作流。');
    const buttons = container.querySelectorAll('button');
    expect(buttons).toHaveLength(2);
    for (const button of buttons) {
      expect(button.dataset.variant).toBe('outline');
      expect(button.dataset.size).toBe('sm');
      expect(button.classList.contains('max-w-full')).toBe(true);
      expect(button.classList.contains('whitespace-normal')).toBe(true);
      const row = button.parentElement!;
      expect(row.classList.contains('flex-wrap')).toBe(true);
      expect(row.querySelector('h3')?.parentElement?.classList.contains('min-w-0')).toBe(true);
      expect(row.querySelector('h3')?.parentElement?.classList.contains('basis-64')).toBe(true);
      const description = document.getElementById(button.getAttribute('aria-describedby') ?? '');
      expect(description?.textContent).toBe(row.querySelector('p')?.textContent);
    }
  });

  it.each([
    ['onboarding.rerun', '#/onboarding'],
    ['onboarding.configureDefaults', '#/onboarding/defaults'],
  ])('keeps the existing %s navigation without applying settings', async (key, route) => {
    const container = await renderSettings();
    const button = [...container.querySelectorAll('button')].find((candidate) => candidate.textContent === i18n.t(key));
    expect(button).toBeDefined();
    await act(async () => button!.click());
    expect(window.location.hash).toBe(route);
  });

  it.each(['zh-CN', 'zh-TW', 'en', 'ja-JP', 'ko-KR', 'pt-BR', 'es'])('renders localized actions in %s', async (locale) => {
    await i18n.changeLanguage(locale);
    const container = await renderSettings();
    for (const key of ['onboarding.rerunTitle', 'onboarding.rerunDescription', 'onboarding.defaultsTitle', 'onboarding.defaultsDescription', 'onboarding.rerun', 'onboarding.configureDefaults']) {
      expect(i18n.t(key)).not.toBe(key);
      expect(container.textContent).toContain(i18n.t(key));
    }
  });
});
