import { useSyncExternalStore } from 'react';

export interface Rgb {
  r: number;
  g: number;
  b: number;
}

/** The few Gold Band theme roles a diagram palette is derived from. */
export interface MermaidThemeTokens {
  background: Rgb;
  foreground: Rgb;
  accent: Rgb;
  accentForeground: Rgb;
  /** Theme warning role; decisions use it so branch points stand out from the accent-tinted nodes. */
  warning: Rgb;
  darkMode: boolean;
}

/** Inputs for Mermaid's customizable `base` theme; `key` identifies the palette for caching. */
export interface MermaidTheme {
  key: string;
  darkMode: boolean;
  background: string;
  variables: Record<string, string>;
  /** Rules the variables cannot express: per-subgraph hues and decision nodes. */
  css: string;
}

const THEME_TOKEN_VARIABLES = {
  background: '--background',
  foreground: '--foreground',
  accent: '--accent',
  accentForeground: '--accent-foreground',
  warning: '--gold-warning',
} as const;

// Root changes that can move the tokens: theme, color scheme, and inline token projection.
const THEME_ROOT_ATTRIBUTES = ['data-theme', 'data-color-scheme', 'style'];

const FALLBACK_TOKENS: Record<'light' | 'dark', Omit<MermaidThemeTokens, 'darkMode'>> = {
  light: {
    background: { r: 255, g: 255, b: 255 },
    foreground: { r: 13, g: 13, b: 13 },
    accent: { r: 232, g: 245, b: 240 },
    accentForeground: { r: 10, g: 122, b: 94 },
    warning: { r: 164, g: 101, b: 8 },
  },
  dark: {
    background: { r: 17, g: 17, b: 17 },
    foreground: { r: 240, g: 240, b: 240 },
    accent: { r: 24, g: 60, b: 51 },
    accentForeground: { r: 125, g: 226, b: 193 },
    warning: { r: 217, g: 133, b: 71 },
  },
};

// Share of the foreground mixed into the background for each neutral role.
const SURFACE_MIX = {
  cluster: 0.025,
  clusterBorder: 0.18,
  border: 0.32,
  line: 0.5,
  actorLine: 0.35,
  accentBorder: 0.5,
};

// Tinted roles mix a saturated ink of their hue into the background, so they sit on any theme surface.
const HUE_INK = { light: { s: 0.65, l: 0.4 }, dark: { s: 0.6, l: 0.62 } };
const TINT_MIX = {
  fill: { light: 0.12, dark: 0.16 },
  clusterFill: { light: 0.07, dark: 0.09 },
  border: 0.5,
  /** Share of the ink in a subgraph title, the rest being the foreground. */
  label: 0.6,
};
// Subgraphs cycle through this many categorical hues, starting after the node hue.
const CLUSTER_HUE_COUNT = 6;

// Categorical series (pie slices, mindmap/timeline sections, git branches). Mermaid derives these by
// rotating the primary hue, which collapses to grey for a neutral palette, so they are generated here.
const CATEGORICAL_COUNT = 12;
// 150° steps visit all twelve 30° hues while keeping neighbouring series far apart.
const CATEGORICAL_HUE_STEP = 150;
const NEUTRAL_ACCENT_SATURATION = 0.15;
const NEUTRAL_ACCENT_HUE = 210;
const CATEGORICAL_TONES = {
  fill: { light: { s: 0.55, l: 0.88 }, dark: { s: 0.35, l: 0.28 } },
  series: { light: { s: 0.55, l: 0.72 }, dark: { s: 0.45, l: 0.45 } },
};

function clampByte(value: number) {
  return Math.min(255, Math.max(0, Math.round(value)));
}

export function rgbHex({ r, g, b }: Rgb) {
  return `#${[r, g, b].map((channel) => clampByte(channel).toString(16).padStart(2, '0')).join('')}`;
}

/** `weight` of `top` over `bottom`, in sRGB. */
function mix(top: Rgb, bottom: Rgb, weight: number): Rgb {
  return {
    r: top.r * weight + bottom.r * (1 - weight),
    g: top.g * weight + bottom.g * (1 - weight),
    b: top.b * weight + bottom.b * (1 - weight),
  };
}

function rgbHsl({ r, g, b }: Rgb) {
  const [red, green, blue] = [r / 255, g / 255, b / 255];
  const max = Math.max(red, green, blue);
  const min = Math.min(red, green, blue);
  const lightness = (max + min) / 2;
  if (max === min) return { h: 0, s: 0, l: lightness };
  const delta = max - min;
  const saturation = lightness > 0.5 ? delta / (2 - max - min) : delta / (max + min);
  const hue = max === red
    ? (green - blue) / delta + (green < blue ? 6 : 0)
    : max === green ? (blue - red) / delta + 2 : (red - green) / delta + 4;
  return { h: hue * 60, s: saturation, l: lightness };
}

function hslRgb(h: number, s: number, l: number): Rgb {
  const chroma = (1 - Math.abs(2 * l - 1)) * s;
  const segment = (((h % 360) + 360) % 360) / 60;
  const secondary = chroma * (1 - Math.abs((segment % 2) - 1));
  const [r, g, b] = segment < 1 ? [chroma, secondary, 0]
    : segment < 2 ? [secondary, chroma, 0]
      : segment < 3 ? [0, chroma, secondary]
        : segment < 4 ? [0, secondary, chroma]
          : segment < 5 ? [secondary, 0, chroma]
            : [chroma, 0, secondary];
  const offset = l - chroma / 2;
  return { r: (r + offset) * 255, g: (g + offset) * 255, b: (b + offset) * 255 };
}

function hueInk(hue: number, scheme: 'light' | 'dark') {
  const { s, l } = HUE_INK[scheme];
  return hslRgb(hue, s, l);
}

function categoricalScale(baseHue: number, tone: { s: number; l: number }) {
  return Array.from({ length: CATEGORICAL_COUNT }, (_, index) =>
    rgbHex(hslRgb(baseHue + index * CATEGORICAL_HUE_STEP, tone.s, tone.l)));
}

/** Derives every Mermaid `base` input from the theme's background, foreground and accent roles. */
export function deriveMermaidTheme(tokens: MermaidThemeTokens): MermaidTheme {
  const { background, foreground, accent, accentForeground, warning, darkMode } = tokens;
  const scheme = darkMode ? 'dark' : 'light';
  const bg = rgbHex(background);
  const fg = rgbHex(foreground);
  const cluster = rgbHex(mix(foreground, background, SURFACE_MIX.cluster));
  const clusterBorder = rgbHex(mix(foreground, background, SURFACE_MIX.clusterBorder));
  const border = rgbHex(mix(foreground, background, SURFACE_MIX.border));
  const line = rgbHex(mix(foreground, background, SURFACE_MIX.line));
  const accentSurface = rgbHex(accent);
  const accentBorder = rgbHex(mix(accentForeground, background, SURFACE_MIX.accentBorder));
  const accentHsl = rgbHsl(accentForeground);
  const neutralAccent = accentHsl.s < NEUTRAL_ACCENT_SATURATION;
  const baseHue = neutralAccent ? NEUTRAL_ACCENT_HUE : accentHsl.h;
  // Nodes carry the theme accent; a neutral accent falls back to the categorical start hue.
  const brandInk = neutralAccent ? hueInk(baseHue, scheme) : accentForeground;
  const node = rgbHex(mix(brandInk, background, TINT_MIX.fill[scheme]));
  const nodeBorder = rgbHex(mix(brandInk, background, TINT_MIX.border));
  const fills = categoricalScale(baseHue, CATEGORICAL_TONES.fill[scheme]);
  const series = categoricalScale(baseHue, CATEGORICAL_TONES.series[scheme]);

  const variables: Record<string, string> = {
    background: bg,
    primaryColor: node,
    primaryBorderColor: nodeBorder,
    primaryTextColor: fg,
    secondaryColor: accentSurface,
    secondaryBorderColor: accentBorder,
    secondaryTextColor: fg,
    tertiaryColor: cluster,
    tertiaryBorderColor: clusterBorder,
    tertiaryTextColor: fg,
    mainBkg: node,
    nodeBorder,
    nodeTextColor: fg,
    textColor: fg,
    titleColor: fg,
    clusterBkg: cluster,
    clusterBorder,
    lineColor: line,
    defaultLinkColor: line,
    arrowheadColor: line,
    edgeLabelBackground: bg,
    noteBkgColor: accentSurface,
    noteBorderColor: accentBorder,
    noteTextColor: fg,
    actorBkg: node,
    actorBorder: nodeBorder,
    actorTextColor: fg,
    actorLineColor: rgbHex(mix(foreground, background, SURFACE_MIX.actorLine)),
    signalColor: fg,
    signalTextColor: fg,
    labelBoxBkgColor: node,
    labelBoxBorderColor: border,
    labelTextColor: fg,
    loopTextColor: fg,
    activationBkgColor: accentSurface,
    activationBorderColor: accentBorder,
    pieTitleTextColor: fg,
    pieSectionTextColor: fg,
    pieLegendTextColor: fg,
    pieStrokeColor: bg,
    pieOuterStrokeColor: border,
  };
  fills.forEach((color, index) => {
    variables[`cScale${index}`] = color;
    variables[`cScaleLabel${index}`] = fg;
    if (index < 8) variables[`fillType${index}`] = color;
  });
  series.forEach((color, index) => {
    variables[`pie${index + 1}`] = color;
    // Mindmap/timeline section accents (edges, underlines) use the stronger tone of the section hue.
    variables[`cScaleInv${index}`] = color;
    if (index < 8) {
      variables[`git${index}`] = color;
      variables[`gitBranchLabel${index}`] = fg;
    }
  });

  const clusterRules = Array.from({ length: CLUSTER_HUE_COUNT }, (_, index) => {
    const ink = hueInk(baseHue + (index + 1) * CATEGORICAL_HUE_STEP, scheme);
    const cluster = `.clusters > .cluster:nth-child(${CLUSTER_HUE_COUNT}n+${index + 1})`;
    return `${cluster} > rect{fill:${rgbHex(mix(ink, background, TINT_MIX.clusterFill[scheme]))};`
      + `stroke:${rgbHex(mix(ink, background, TINT_MIX.border))};}`
      + `${cluster} text{fill:${rgbHex(mix(ink, foreground, TINT_MIX.label))};}`;
  });
  const css = [
    `.node polygon{fill:${rgbHex(mix(warning, background, TINT_MIX.fill[scheme]))};`
      + `stroke:${rgbHex(mix(warning, background, TINT_MIX.border))};}`,
    ...clusterRules,
  ].join('');

  return {
    key: [scheme, bg, fg, accentSurface, rgbHex(accentForeground), rgbHex(warning)].join(':'),
    darkMode,
    background: bg,
    variables,
    css,
  };
}

const HEX_COLOR = /^#([\da-f]{3}|[\da-f]{6})$/iu;
const RGB_COLOR = /^rgba?\(\s*([\d.]+)[\s,]+([\d.]+)[\s,]+([\d.]+)(?:[\s,/]+([\d.]+%?))?\s*\)$/iu;

function parseSimpleColor(value: string): (Rgb & { a: number }) | null {
  const hex = value.match(HEX_COLOR)?.[1];
  if (hex) {
    const full = hex.length === 3 ? [...hex].map((digit) => digit + digit).join('') : hex;
    return {
      r: Number.parseInt(full.slice(0, 2), 16),
      g: Number.parseInt(full.slice(2, 4), 16),
      b: Number.parseInt(full.slice(4, 6), 16),
      a: 1,
    };
  }
  const rgb = value.match(RGB_COLOR);
  if (!rgb) return null;
  const alpha = rgb[4] === undefined ? 1 : rgb[4].endsWith('%') ? Number.parseFloat(rgb[4]) / 100 : Number(rgb[4]);
  return { r: Number(rgb[1]), g: Number(rgb[2]), b: Number(rgb[3]), a: alpha };
}

let colorProbe: OffscreenCanvasRenderingContext2D | null | undefined;

/**
 * Any CSS color a theme may use (hex, rgb, hsl, oklch, color-mix…) as sRGB, via the browser's own
 * parser; translucent values are composited over `backdrop`.
 */
function resolveCssColor(value: string, backdrop: Rgb | null): Rgb | null {
  const trimmed = value.trim();
  if (!trimmed) return null;
  let color = parseSimpleColor(trimmed);
  if (!color) {
    if (colorProbe === undefined) {
      colorProbe = typeof OffscreenCanvas === 'undefined'
        ? null
        : new OffscreenCanvas(1, 1).getContext('2d', { willReadFrequently: true });
    }
    if (!colorProbe) return null;
    // An invalid value leaves fillStyle untouched, so two different sentinels detect it.
    colorProbe.fillStyle = '#000000';
    colorProbe.fillStyle = trimmed;
    const first = colorProbe.fillStyle;
    colorProbe.fillStyle = '#ffffff';
    colorProbe.fillStyle = trimmed;
    if (first !== colorProbe.fillStyle) return null;
    colorProbe.clearRect(0, 0, 1, 1);
    colorProbe.fillRect(0, 0, 1, 1);
    const [r, g, b, a] = colorProbe.getImageData(0, 0, 1, 1).data;
    color = { r, g, b, a: a / 255 };
  }
  const { a: alpha, ...rgb } = color;
  return backdrop && alpha < 1 ? mix(rgb, backdrop, alpha) : rgb;
}

export function readMermaidThemeTokens(): MermaidThemeTokens {
  const root = document.documentElement;
  const darkMode = root.getAttribute('data-color-scheme') === 'dark';
  const fallback = FALLBACK_TOKENS[darkMode ? 'dark' : 'light'];
  const style = getComputedStyle(root);
  const read = (name: keyof typeof THEME_TOKEN_VARIABLES, backdrop: Rgb | null) =>
    resolveCssColor(style.getPropertyValue(THEME_TOKEN_VARIABLES[name]), backdrop) ?? fallback[name];
  const background = read('background', null);
  return {
    background,
    foreground: read('foreground', background),
    accent: read('accent', background),
    accentForeground: read('accentForeground', background),
    warning: read('warning', background),
    darkMode,
  };
}

const listeners = new Set<() => void>();
let observer: MutationObserver | null = null;
let snapshot: MermaidTheme | null = null;
const serverSnapshot = deriveMermaidTheme({ ...FALLBACK_TOKENS.light, darkMode: false });

function refreshSnapshot() {
  const next = deriveMermaidTheme(readMermaidThemeTokens());
  if (snapshot?.key === next.key) return false;
  snapshot = next;
  return true;
}

function getSnapshot() {
  if (typeof document === 'undefined') return serverSnapshot;
  if (!snapshot || !observer) refreshSnapshot();
  return snapshot!;
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  if (!observer && typeof MutationObserver !== 'undefined' && typeof document !== 'undefined') {
    observer = new MutationObserver(() => {
      if (refreshSnapshot()) listeners.forEach((notify) => notify());
    });
    observer.observe(document.documentElement, { attributes: true, attributeFilter: THEME_ROOT_ATTRIBUTES });
    refreshSnapshot();
  }
  return () => {
    listeners.delete(listener);
    if (listeners.size === 0) {
      observer?.disconnect();
      observer = null;
    }
  };
}

/** Diagram palette following the current Gold Band theme; identity changes only when the palette does. */
export function useMermaidTheme(): MermaidTheme {
  return useSyncExternalStore(subscribe, getSnapshot, () => serverSnapshot);
}
