import { Info, Lightbulb, MessageSquareWarning, OctagonAlert, TriangleAlert, type LucideIcon } from 'lucide-react';

/** GitHub alert types (`> [!NOTE]` …). Other `[!…]` markers stay plain blockquotes. */
export const MARKDOWN_ALERT_TYPES = ['note', 'tip', 'important', 'warning', 'caution'] as const;
export type MarkdownAlertType = typeof MARKDOWN_ALERT_TYPES[number];

/** `className` styles React alerts; `colorVar` styles non-React renderers such as CodeMirror. */
export const markdownAlertStyles: Record<MarkdownAlertType, { icon: LucideIcon; className: string; colorVar: string }> = {
  note: { icon: Info, className: 'border-gold-attention text-gold-attention', colorVar: 'var(--gold-attention)' },
  tip: { icon: Lightbulb, className: 'border-gold-success text-gold-success', colorVar: 'var(--gold-success)' },
  important: { icon: MessageSquareWarning, className: 'border-gold-emphasis text-gold-emphasis', colorVar: 'var(--gold-emphasis)' },
  warning: { icon: TriangleAlert, className: 'border-gold-warning text-gold-warning', colorVar: 'var(--gold-warning)' },
  caution: { icon: OctagonAlert, className: 'border-gold-danger text-gold-danger', colorVar: 'var(--gold-danger)' },
};

export function markdownAlertTitleKey(type: MarkdownAlertType) {
  return `common.markdownAlert.${type}` as const;
}

const ALERT_MARKER_PATTERN = /^\[!([a-z]+)\]$/iu;

/** Parses the exact `[!TYPE]` marker text; matching is case-insensitive like GitHub. */
export function markdownAlertTypeFromMarker(marker: string): MarkdownAlertType | null {
  const type = marker.match(ALERT_MARKER_PATTERN)?.[1]?.toLowerCase();
  return MARKDOWN_ALERT_TYPES.find((candidate) => candidate === type) ?? null;
}
