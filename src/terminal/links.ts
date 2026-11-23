import { openUrl } from "@tauri-apps/plugin-opener";
import type { GhosttyCore } from "@wterm/ghostty";
import { useStore } from "../store/store";
import { type TermRow, urlAt } from "./urlAt";

/** Opens the http(s) link under a Cmd-click; `detail === 0` is a keyboard-activated OSC 8 link. */
export function openClickedLink(event: React.MouseEvent, core: GhosttyCore | null): void {
  if (!(event.target instanceof Element)) return;
  const modifier = useStore.getState().platform === "mac" ? event.metaKey : event.ctrlKey;
  const link = event.target.closest<HTMLAnchorElement>("a.term-link");
  if (link) {
    if (!modifier && event.detail !== 0) return;
    event.preventDefault();
    if (/^https?:\/\//i.test(link.href)) void openUrl(link.href);
    return;
  }
  if (!modifier || !core) return;
  const url = plainUrlAt(event.target, event.clientX, event.clientY, core);
  if (url) {
    event.preventDefault();
    void openUrl(url);
  }
}

function plainUrlAt(target: Element, x: number, y: number, core: GhosttyCore): string | undefined {
  const rowEl = target.closest(".term-row");
  const screen = rowEl?.parentElement;
  const caret = document.caretRangeFromPoint?.(x, y);
  if (!rowEl || !screen || !caret || !rowEl.contains(caret.startContainer)) return undefined;
  const prefix = document.createRange();
  prefix.selectNodeContents(rowEl);
  prefix.setEnd(caret.startContainer, caret.startOffset);

  const elements = Array.from(screen.querySelectorAll(":scope > .term-row"));
  return urlAt(rowsFromDom(elements, core), elements.indexOf(rowEl), prefix.toString().length);
}

/** Live rows carry wterm's wrap flag; scrollback rows count as wrapped when full-width. */
function rowsFromDom(elements: Element[], core: GhosttyCore): TermRow[] {
  const cols = core.getCols();
  let liveRow = 0;
  return elements.map((element) => {
    const text = element.textContent ?? "";
    const wrapsToNext = element.classList.contains("term-scrollback-row")
      ? [...text].length >= cols && adjacentRow(element)
      : (core.getRowMetadata?.(liveRow++)?.wrapsToNext ?? false);
    return { text, wrapsToNext };
  });
}

/** Whether the next mounted row is also the next terminal row: empty spacers don't hide any. */
function adjacentRow(element: Element): boolean {
  let next = element.nextElementSibling;
  while (next instanceof HTMLElement && next.classList.contains("term-scrollback-spacer")) {
    if (next.offsetHeight > 0) return false;
    next = next.nextElementSibling;
  }
  return !!next?.classList.contains("term-row");
}
