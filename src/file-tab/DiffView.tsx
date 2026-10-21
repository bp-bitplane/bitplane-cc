/**
 * The only place that touches `@pierre/diffs` (ADR 0009): swapping the
 * library means rewriting this file, not its callers. Loaded lazily with the
 * first File tab. `highlightCode` also serves the Markdown preview's code
 * blocks, so `MarkdownView`'s chunk pulls this module in.
 */
import {
  type FileDiffMetadata,
  getSharedHighlighter,
  parsePatchFiles,
  type ThemedToken,
} from "@pierre/diffs";
import { File, FileDiff } from "@pierre/diffs/react";
import { useMemo } from "react";
import type { FileContents } from "../ipc";
import type { DiffStyle } from "../store/store";
import theme from "../ui/pierreTheme.module.css";
import styles from "./DiffView.module.css";

export type DiffViewProps =
  | {
      kind: "patch";
      patch: string;
      diffStyle: DiffStyle;
      /** Both sides in full, fetched only when unchanged context is expanded. */
      loadContents: () => Promise<FileContents>;
    }
  | { kind: "file"; name: string; contents: string };

const COMMON = {
  themeType: "dark",
  overflow: "scroll",
  disableFileHeader: true,
  // Drag-selecting with it on crashes WKWebView (pierre#1184).
  enableGutterUtility: false,
} as const;

/** What `<File themeType="dark">` resolves to, so code blocks match the source view. */
const CODE_THEME = "pierre-dark";

export default function DiffView(props: DiffViewProps) {
  return props.kind === "patch" ? (
    <PatchView {...props} />
  ) : (
    <File
      file={{ name: props.name, contents: props.contents }}
      options={COMMON}
      className={`${styles.view} ${theme.diffs}`}
    />
  );
}

/**
 * Shiki tokens for a code block, from the highlighter `<File>` already uses;
 * `null` for no language or one Shiki doesn't know, so it stays plain text.
 */
export async function highlightCode(code: string, lang: string): Promise<ThemedToken[][] | null> {
  if (!lang) return null;
  try {
    const highlighter = await getSharedHighlighter({ themes: [CODE_THEME], langs: [lang] });
    return highlighter.codeToTokens(code, { lang, theme: CODE_THEME }).tokens;
  } catch {
    return null;
  }
}

function PatchView({ patch, diffStyle, loadContents }: Extract<DiffViewProps, { kind: "patch" }>) {
  const fileDiff = useMemo(() => parse(patch), [patch]);
  const options = useMemo(
    () => ({
      ...COMMON,
      diffStyle,
      lineDiffType: "word" as const,
      loadDiffFiles: async (diff: FileDiffMetadata) => {
        const contents = await loadContents();
        const newFile = { name: diff.name, contents: contents.new };
        return diff.type === "rename-pure"
          ? { oldFile: null, newFile }
          : { oldFile: { name: diff.prevName ?? diff.name, contents: contents.old }, newFile };
      },
    }),
    [diffStyle, loadContents],
  );
  if (!fileDiff) return <pre className={styles.raw}>{patch}</pre>;
  return (
    <FileDiff fileDiff={fileDiff} options={options} className={`${styles.view} ${theme.diffs}`} />
  );
}

/** `null` when the patch doesn't parse, so the raw text shows instead. */
function parse(patch: string): FileDiffMetadata | null {
  try {
    return parsePatchFiles(patch, undefined, true)[0]?.files[0] ?? null;
  } catch {
    return null;
  }
}
