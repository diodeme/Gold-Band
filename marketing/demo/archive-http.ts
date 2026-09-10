export async function boundedBytes(response: Response, expected: number, exactLength = true) {
  const invalid = () => ({ code: 'demo.archive-range-invalid', params: {} });
  const reader = response.body?.getReader();
  if (!reader) { if (expected === 0) return new Uint8Array(); throw invalid(); }
  const chunks: Uint8Array[] = [];
  let bytes = 0;
  while (true) {
    const part = await reader.read();
    if (part.done) break;
    bytes += part.value.byteLength;
    if (bytes > expected) { await reader.cancel(); throw invalid(); }
    chunks.push(part.value);
  }
  if (exactLength && bytes !== expected) throw invalid();
  const buffer = new Uint8Array(bytes);
  let position = 0;
  for (const chunk of chunks) { buffer.set(chunk, position); position += chunk.byteLength; }
  return buffer;
}
