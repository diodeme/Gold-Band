// Snapshot fields and inline styles must resolve assets on the deployment origin.
export function portableRecording(recording, captureOrigin) {
  const origin = new URL(captureOrigin).origin;
  function visit(value) {
    if (typeof value === 'string') return value.replaceAll(`${origin}/`, '/');
    if (Array.isArray(value)) return value.map(visit);
    if (value && typeof value === 'object') return Object.fromEntries(Object.entries(value).map(([key, child]) => [key, visit(child)]));
    return value;
  }
  const result = visit(recording);
  result.bytes = new TextEncoder().encode(JSON.stringify(result.events)).length;
  return result;
}
