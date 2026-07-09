import { cn } from "@/lib/utils";

export const GOLD_THEMED_SCROLLBAR_CLASS = "gold-themed-scrollbar";

export function goldThemedScrollbarClassName(...classNames: Array<string | null | undefined | false>) {
  return cn(GOLD_THEMED_SCROLLBAR_CLASS, ...classNames);
}
