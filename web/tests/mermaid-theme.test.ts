/** @vitest-environment jsdom */

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, expect, it } from 'vitest';
import {
  type MermaidTheme,
  deriveMermaidTheme,
  readMermaidThemeTokens,
  useMermaidTheme,
  type MermaidThemeTokens,
} from '@/lib/mermaid-theme';

const HEX = /^#[\da-f]{6}$/u;

const lightTokens: MermaidThemeTokens = {
  background: { r: 255, g: 255, b: 255 },
  foreground: { r: 13, g: 13, b: 13 },
  accent: { r: 232, g: 245, b: 240 },
  accentForeground: { r: 10, g: 122, b: 94 },
  warning: { r: 164, g: 101, b: 8 },
  darkMode: false,
};
const neutralDarkTokens: MermaidThemeTokens = {
  background: { r: 15, g: 15, b: 15 },
  foreground: { r: 232, g: 232, b: 232 },
  accent: { r: 38, g: 38, b: 38 },
  accentForeground: { r: 238, g: 238, b: 238 },
  warning: { r: 217, g: 133, b: 71 },
  darkMode: true,
};

function luminance(hex: string) {
  const value = Number.parseInt(hex.slice(1), 16);
  return ((value >> 16) & 255) * 0.299 + ((value >> 8) & 255) * 0.587 + (value & 255) * 0.114;
}

function isGrey(hex: string) {
  const value = Number.parseInt(hex.slice(1), 16);
  const channels = [(value >> 16) & 255, (value >> 8) & 255, value & 255];
  return Math.max(...channels) - Math.min(...channels) < 12;
}

afterEach(() => {
  document.documentElement.removeAttribute('data-color-scheme');
  document.documentElement.removeAttribute('style');
});

it('derives a deterministic, hex-only base palette from the theme roles', () => {
  const theme = deriveMermaidTheme(lightTokens);
  expect(deriveMermaidTheme(lightTokens)).toEqual(theme);
  expect(theme.background).toBe('#ffffff');
  expect(theme.darkMode).toBe(false);
  Object.values(theme.variables).forEach((value) => expect(value).toMatch(HEX));
  expect(theme.variables.primaryTextColor).toBe('#0d0d0d');
  expect(theme.variables.edgeLabelBackground).toBe('#ffffff');
  expect(theme.variables.secondaryColor).toBe('#e8f5f0');
  // Nodes are a light tint of the accent; edges stay neutral and darker than node borders.
  const bg = luminance(theme.background);
  expect(Math.abs(luminance(theme.variables.primaryColor) - bg)).toBeLessThan(30);
  expect(isGrey(theme.variables.primaryColor)).toBe(false);
  expect(isGrey(theme.variables.lineColor)).toBe(true);
  expect(luminance(theme.variables.lineColor)).toBeLessThan(luminance(theme.variables.primaryBorderColor));
});

it('tints nodes with a fallback hue and gives subgraphs and decisions their own hues', () => {
  const theme = deriveMermaidTheme(neutralDarkTokens);
  // A grey accent still yields colored nodes.
  expect(isGrey(theme.variables.mainBkg)).toBe(false);
  expect(isGrey(theme.variables.nodeBorder)).toBe(false);
  const clusterFills = [...theme.css.matchAll(/\.cluster:nth-child\(6n\+\d\) > rect\{fill:(#[\da-f]{6})/gu)].map((match) => match[1]);
  expect(clusterFills).toHaveLength(6);
  expect(new Set(clusterFills).size).toBe(6);
  clusterFills.forEach((color) => expect(color).not.toBe(theme.background));
  const clusterStrokes = [...theme.css.matchAll(/> rect\{fill:#[\da-f]{6};stroke:(#[\da-f]{6})/gu)].map((match) => match[1]);
  expect(clusterStrokes).toHaveLength(6);
  clusterStrokes.forEach((color) => expect(isGrey(color)).toBe(false));
  const decisionFill = theme.css.match(/\.node polygon\{fill:(#[\da-f]{6})/u)?.[1];
  expect(decisionFill).toBeDefined();
  expect(decisionFill).not.toBe(theme.variables.mainBkg);
  // Decisions follow the theme's warning role.
  const recolored = deriveMermaidTheme({ ...neutralDarkTokens, warning: { r: 180, g: 40, b: 160 } });
  expect(recolored.css.match(/\.node polygon\{fill:(#[\da-f]{6})/u)?.[1]).not.toBe(decisionFill);
  expect(deriveMermaidTheme(neutralDarkTokens).css).toBe(theme.css);
});

it('keeps categorical series distinct and colored even for a fully neutral theme', () => {
  const theme = deriveMermaidTheme(neutralDarkTokens);
  const fills = Array.from({ length: 12 }, (_, index) => theme.variables[`cScale${index}`]);
  const series = Array.from({ length: 12 }, (_, index) => theme.variables[`pie${index + 1}`]);
  expect(new Set(fills).size).toBe(12);
  expect(new Set(series).size).toBe(12);
  [...fills, ...series].forEach((color) => expect(isGrey(color)).toBe(false));
  // Dark fills stay dark enough for the light label color.
  fills.forEach((color) => expect(luminance(color)).toBeLessThan(110));
  expect(theme.variables.cScaleLabel0).toBe('#e8e8e8');
  expect(theme.variables.git0).toBe(theme.variables.pie1);
});

it('changes the palette key only when a theme role changes', () => {
  const base = deriveMermaidTheme(lightTokens);
  expect(deriveMermaidTheme({ ...lightTokens }).key).toBe(base.key);
  expect(deriveMermaidTheme({ ...lightTokens, accentForeground: { r: 200, g: 60, b: 20 } }).key).not.toBe(base.key);
  expect(deriveMermaidTheme({ ...lightTokens, darkMode: true }).key).not.toBe(base.key);
  expect(deriveMermaidTheme({ ...lightTokens, warning: { r: 180, g: 40, b: 160 } }).key).not.toBe(base.key);
});

it('reads theme tokens from the root, compositing translucent colors over the background', () => {
  const root = document.documentElement;
  root.setAttribute('data-color-scheme', 'dark');
  root.style.setProperty('--background', '#101010');
  root.style.setProperty('--foreground', 'rgb(240 240 240)');
  root.style.setProperty('--accent', 'rgba(255, 255, 255, 0.5)');
  root.style.setProperty('--accent-foreground', '#7de2c1');
  root.style.setProperty('--gold-warning', '#d98547');
  const tokens = readMermaidThemeTokens();
  expect(tokens.darkMode).toBe(true);
  expect(tokens.background).toEqual({ r: 16, g: 16, b: 16 });
  expect(tokens.foreground).toEqual({ r: 240, g: 240, b: 240 });
  expect(tokens.accent).toEqual({ r: 135.5, g: 135.5, b: 135.5 });
  expect(tokens.accentForeground).toEqual({ r: 125, g: 226, b: 193 });
  expect(tokens.warning).toEqual({ r: 217, g: 133, b: 71 });
});

it('publishes a new palette when the root theme changes and keeps identity otherwise', async () => {
  const root = document.documentElement;
  root.style.setProperty('--background', '#ffffff');
  root.style.setProperty('--foreground', '#0d0d0d');
  const seen: MermaidTheme[] = [];
  function Probe() {
    seen.push(useMermaidTheme());
    return null;
  }
  const root2 = createRoot(document.createElement('div'));
  await act(async () => root2.render(React.createElement(Probe)));
  const result = { get current() { return seen[seen.length - 1]; } };
  const initial = result.current;
  expect(initial.darkMode).toBe(false);

  await act(async () => {
    root.setAttribute('data-theme', 'same-colors');
    await Promise.resolve();
  });
  expect(result.current).toBe(initial);

  await act(async () => {
    root.setAttribute('data-color-scheme', 'dark');
    root.style.setProperty('--background', '#111111');
    root.style.setProperty('--foreground', '#f0f0f0');
    await Promise.resolve();
  });
  expect(result.current).not.toBe(initial);
  expect(result.current.darkMode).toBe(true);
  expect(result.current.background).toBe('#111111');
  root.removeAttribute('data-theme');
  act(() => root2.unmount());
});
