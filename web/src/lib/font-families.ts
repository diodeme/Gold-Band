const fontCatalogCollator = new Intl.Collator(['zh-CN', 'en'], {
  sensitivity: 'base',
  numeric: true,
});

export function normalizeFontCatalogFamilies(families: readonly string[]) {
  const seen = new Set<string>();
  const normalized: string[] = [];
  for (const family of families) {
    const trimmed = family.trim();
    const key = trimmed.toLocaleLowerCase();
    if (!trimmed || seen.has(key)) continue;
    seen.add(key);
    normalized.push(trimmed);
  }
  return normalized.sort((left, right) => fontCatalogCollator.compare(left, right));
}
