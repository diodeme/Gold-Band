export function isImageMime(mime: string): boolean {
  return mime.startsWith("image/") && !mime.includes("svg");
}

/** Whether the workspace can show the attachment as text; any file type can be attached. */
export function isTextPreviewableMime(mime: string): boolean {
  const type = mime.split(";", 1)[0].trim().toLowerCase();
  return type.startsWith("text/") || ["application/json", "application/jsonl", "application/xml"].includes(type);
}
