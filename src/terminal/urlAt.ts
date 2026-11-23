export interface TermRow {
  text: string;
  /** The next row continues this one without a newline (a soft wrap). */
  wrapsToNext: boolean;
}

/**
 * The http(s) URL at `offset` in `rows[row]`, read across soft-wrapped rows.
 * Delete once wterm ships linkify (vercel-labs/wterm#42).
 */
export function urlAt(rows: readonly TermRow[], row: number, offset: number): string | undefined {
  if (!rows[row]) return undefined;
  let first = row;
  while (rows[first - 1]?.wrapsToNext) first--;
  let last = row;
  while (rows[last]?.wrapsToNext && rows[last + 1]) last++;

  const preceding = rows.slice(first, row).map((r) => r.text);
  const text = rows
    .slice(first, last + 1)
    .map((r) => r.text)
    .join("");
  const hit = preceding.join("").length + offset;

  for (const match of text.matchAll(URL_CANDIDATE)) {
    const url = trimEnd(match[0]);
    const start = match.index;
    // `offset` is a caret position, so the boundary right after the URL still hits it.
    if (hit < start || hit > start + url.length) continue;
    return url.endsWith("//") ? undefined : url; // "https://" alone is no URL
  }
  return undefined;
}

// A run of non-blank characters starting with a standalone http(s) scheme.
const URL_CANDIDATE = /\bhttps?:\/\/\S+/gi;

const OPENING: Record<string, string> = { ")": "(", "]": "[", "}": "{", ">": "<" };

function trimEnd(candidate: string): string {
  let url = candidate;
  for (;;) {
    const last = url.at(-1) ?? "";
    if (".,;:!?".includes(last) || !balanced(url, last)) url = url.slice(0, -1);
    else return url;
  }
}

function balanced(url: string, last: string): boolean {
  const count = (char: string) => url.split(char).length - 1;
  if (last === '"' || last === "'") return count(last) % 2 === 0;
  const opening = OPENING[last];
  return opening === undefined || count(last) <= count(opening);
}
