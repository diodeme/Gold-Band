export function runtimeStatusDotClass(tone?: string | null) {
  if (tone === 'success') return 'bg-emerald-500';
  if (tone === 'danger') return 'bg-red-500';
  if (tone === 'running') return 'bg-gold-running motion-safe:animate-pulse';
  if (tone === 'warning') return 'bg-yellow-500';
  return 'bg-muted-foreground';
}
