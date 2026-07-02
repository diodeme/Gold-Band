export type ParsedPromptPart =
  | { type: "visible"; text: string }
  | { type: "hidden"; title?: string; text: string };

const HIDDEN_CLOSE = "</hidden>";

export function parseGoldBandHiddenSections(content: string): ParsedPromptPart[] {
  if (!content.includes("<hidden")) {
    return content ? [{ type: "visible", text: content }] : [];
  }

  const parts: ParsedPromptPart[] = [];
  let cursor = 0;

  while (cursor < content.length) {
    const openStart = content.indexOf("<hidden", cursor);
    if (openStart === -1) {
      pushVisible(parts, content.slice(cursor));
      break;
    }

    const openEnd = content.indexOf(">", openStart);
    if (openEnd === -1) {
      return [{ type: "visible", text: content }];
    }

    const openingTag = content.slice(openStart, openEnd + 1);
    const closeStart = content.indexOf(HIDDEN_CLOSE, openEnd + 1);
    const isGoldBandHidden = /\bdata-gold-band-hidden\s*=\s*(?:"true"|'true'|true)(?=\s|>)/i.test(openingTag);

    if (closeStart === -1) {
      if (isGoldBandHidden) {
        return [{ type: "visible", text: content }];
      }
      pushVisible(parts, content.slice(cursor));
      break;
    }

    const closeEnd = closeStart + HIDDEN_CLOSE.length;
    if (!isGoldBandHidden) {
      pushVisible(parts, content.slice(cursor, closeEnd));
      cursor = closeEnd;
      continue;
    }

    pushVisible(parts, content.slice(cursor, openStart));
    parts.push({
      type: "hidden",
      title: hiddenTitleFromTag(openingTag),
      text: content.slice(openEnd + 1, closeStart).replace(/<\\\/hidden>/g, HIDDEN_CLOSE),
    });
    cursor = closeEnd;
  }

  return parts.length > 0 ? parts : [{ type: "visible", text: content }];
}

function pushVisible(parts: ParsedPromptPart[], text: string) {
  if (!text) return;
  const previous = parts.at(-1);
  if (previous?.type === "visible") {
    previous.text += text;
  } else {
    parts.push({ type: "visible", text });
  }
}

function hiddenTitleFromTag(tag: string): string | undefined {
  const match = tag.match(/\btitle\s*=\s*(?:"([^"]*)"|'([^']*)')/i);
  const title = match?.[1] ?? match?.[2];
  return title?.trim() || undefined;
}
