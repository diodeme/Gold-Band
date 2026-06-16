import { useEffect, useState } from "react";
import { getSupportedAttachmentExtensions } from "@/api";

// ── Pure helpers (no data) ──

export function attachmentExt(name: string): string {
  const dot = name.lastIndexOf(".");
  return dot === -1 ? "" : name.slice(dot + 1).toLowerCase();
}

export function isAllowedAttachment(name: string, allowed: Set<string>): boolean {
  const ext = attachmentExt(name);
  return ext !== "" && allowed.has(ext);
}

export function isImageMime(mime: string): boolean {
  return mime.startsWith("image/") && !mime.includes("svg");
}

// ── Fetch extensions from backend (single source of truth) ──

let cachedExtensions: Set<string> | null = null;
let pendingPromise: Promise<Set<string>> | null = null;

async function fetchExtensions(): Promise<Set<string>> {
  if (cachedExtensions) return cachedExtensions;
  if (!pendingPromise) {
    pendingPromise = getSupportedAttachmentExtensions().then((exts) => {
      cachedExtensions = new Set(exts);
      return cachedExtensions;
    });
  }
  return pendingPromise;
}

export function useAttachmentExtensions(): Set<string> | null {
  const [exts, setExts] = useState<Set<string> | null>(cachedExtensions);
  useEffect(() => {
    if (!cachedExtensions) {
      fetchExtensions().then(setExts);
    }
  }, []);
  return exts;
}
