import { ExternalLink } from "lucide-react";
import { lazy, type ReactNode, Suspense, useCallback, useEffect, useState } from "react";
import { commands, type FileContents, type FileDiff, type FileText } from "../ipc";
import { findPlane } from "../store/selectors";
import { type DiffStyle, onFileTabsSignal, openFile, setDiffStyle, useStore } from "../store/store";
import { type FileTab, fileTabTitle } from "../tabs/fileTabs";
import { Centered } from "../ui/Centered";
import styles from "./FileTabView.module.css";
import { changedLines, firstChangedLine, MAX_CHANGED_LINES, renameOf } from "./patch";

const DiffView = lazy(() => import("./DiffView"));
const MarkdownView = lazy(() => import("./MarkdownView"));

type Content =
  | { kind: "loading" }
  | { kind: "error"; message: string }
  | { kind: "diff"; diff: Exclude<FileDiff, { kind: "plain" }> }
  | { kind: "plain"; text: FileText };

/** A File tab: its diff or plain contents, a header, and every state in between. */
export function FileTabView({ tab }: { tab: FileTab }) {
  const { planeId, path, mode } = tab;
  const planeKind = useStore((s) => findPlane(s, planeId)?.kind);
  const diffStyle = useStore((s) => s.diffStyle);
  const [content, setContent] = useState<Content>({ kind: "loading" });
  const [force, setForce] = useState(false);
  const [showAnyway, setShowAnyway] = useState(false);
  /** Markdown preview instead of source; the tab's own, never saved. */
  const [preview, setPreview] = useState(false);
  /** Bumped when git or the file changed; the last content stays up meanwhile. */
  const [version, setVersion] = useState(0);
  const available = planeKind !== "gone" && planeKind !== "creating";

  useEffect(
    () => onFileTabsSignal((changed) => changed === planeId && setVersion((v) => v + 1)),
    [planeId],
  );

  useEffect(() => {
    if (!available) return;
    void version;
    let current = true;
    void load(planeId, path, mode, force).then((loaded) => {
      if (current) setContent(loaded);
    });
    return () => {
      current = false;
    };
  }, [available, planeId, path, mode, force, version]);

  const loadContents = useCallback(
    () => commands.planeFileContents(planeId, path, mode),
    [planeId, path, mode],
  );

  if (!available) return <PlaneNotice planeId={planeId} creating={planeKind === "creating"} />;

  const patch = content.kind === "diff" && content.diff.kind === "diff" ? content.diff.patch : null;
  const rename = patch ? renameOf(patch) : null;
  const line = patch ? firstChangedLine(patch) : undefined;
  const markdown =
    isMarkdown(path) && content.kind === "plain" && content.text.kind === "text"
      ? content.text.contents
      : null;
  const open = () => void openFile(planeId, path, line);
  const openButton = (
    <button type="button" onClick={open}>
      Open in VS Code
    </button>
  );

  return (
    <div className={styles.view}>
      <header className={styles.header}>
        <span className={styles.path} title={path}>
          {rename ? `${rename.from} → ${rename.to}` : path}
        </span>
        {markdown !== null && (
          <fieldset className={styles.toggle} aria-label="Markdown view">
            <button type="button" aria-pressed={!preview} onClick={() => setPreview(false)}>
              Source
            </button>
            <button type="button" aria-pressed={preview} onClick={() => setPreview(true)}>
              Preview
            </button>
          </fieldset>
        )}
        {patch && (
          <fieldset className={styles.toggle} aria-label="Diff layout">
            {(["unified", "split"] as const).map((style) => (
              <button
                key={style}
                type="button"
                aria-pressed={diffStyle === style}
                onClick={() => setDiffStyle(style)}
              >
                {style === "unified" ? "Unified" : "Split"}
              </button>
            ))}
          </fieldset>
        )}
        <button
          type="button"
          className={styles.icon}
          title="Open in VS Code"
          aria-label="Open in VS Code"
          onClick={open}
        >
          <ExternalLink size={14} />
        </button>
      </header>
      <div className={styles.body}>
        {preview && markdown !== null ? (
          <Suspense fallback={null}>
            <MarkdownView source={markdown} />
          </Suspense>
        ) : (
          <Body
            tab={tab}
            content={content}
            showAnyway={showAnyway}
            openButton={openButton}
            onForce={() => setForce(true)}
            onShowAnyway={() => setShowAnyway(true)}
            diffStyle={diffStyle}
            loadContents={loadContents}
          />
        )}
      </div>
    </div>
  );
}

function Body({
  tab,
  content,
  showAnyway,
  openButton,
  onForce,
  onShowAnyway,
  diffStyle,
  loadContents,
}: {
  tab: FileTab;
  content: Content;
  showAnyway: boolean;
  openButton: ReactNode;
  /** Past the backend's size limit: read the diff again without it. */
  onForce: () => void;
  /** Past the changed-line limit: render the diff already read. */
  onShowAnyway: () => void;
  diffStyle: DiffStyle;
  loadContents: () => Promise<FileContents>;
}) {
  const title = fileTabTitle(tab);
  switch (content.kind) {
    case "loading":
      return null;
    case "error":
      return (
        <Centered title={`Couldn't open ${title}`}>
          <p>{content.message}</p>
        </Centered>
      );
    case "plain":
      switch (content.text.kind) {
        case "text":
          return (
            <Suspense fallback={null}>
              <DiffView kind="file" name={tab.path} contents={content.text.contents} />
            </Suspense>
          );
        case "binary":
          return (
            <Centered title="Binary file" actions={openButton}>
              <p>{formatSize(content.text.size)}</p>
            </Centered>
          );
        case "tooLarge":
          return (
            <Centered title="Too large to show" actions={openButton}>
              <p>{formatSize(content.text.size)}</p>
            </Centered>
          );
      }
      return null;
    case "diff": {
      const { diff } = content;
      switch (diff.kind) {
        case "none":
          return (
            <Centered title="No changes">
              <p>
                {tab.path} has no {tab.mode === "index" ? "staged" : "unstaged"} changes anymore.
              </p>
            </Centered>
          );
        case "binary":
          return (
            <Centered title="Binary file" actions={openButton}>
              <p>
                {formatSize(diff.oldSize)} → {formatSize(diff.newSize)}
              </p>
            </Centered>
          );
        case "tooLarge":
          return (
            <TooLarge openButton={openButton} onShowAnyway={onForce}>
              {formatSize(diff.size)}
            </TooLarge>
          );
        case "diff": {
          const lines = changedLines(diff.patch);
          if (lines > MAX_CHANGED_LINES && !showAnyway) {
            return (
              <TooLarge openButton={openButton} onShowAnyway={onShowAnyway}>
                {lines.toLocaleString("en-US")} changed lines
              </TooLarge>
            );
          }
          return (
            <Suspense fallback={null}>
              <DiffView
                kind="patch"
                patch={diff.patch}
                diffStyle={diffStyle}
                loadContents={loadContents}
              />
            </Suspense>
          );
        }
      }
    }
  }
}

function TooLarge({
  children,
  openButton,
  onShowAnyway,
}: {
  children: ReactNode;
  openButton: ReactNode;
  onShowAnyway: () => void;
}) {
  return (
    <Centered
      title="Too large to show"
      actions={
        <>
          {openButton}
          <button type="button" onClick={onShowAnyway}>
            Show anyway
          </button>
        </>
      }
    >
      <p>{children}</p>
    </Centered>
  );
}

/** The Tool panel's notices, for a plane the File tab can't read. */
function PlaneNotice({ planeId, creating }: { planeId: string; creating: boolean }) {
  return creating ? (
    <Centered title={`${planeId} is being created`}>
      <p>Files are available once it's ready.</p>
    </Centered>
  ) : (
    <Centered title={`${planeId} is gone`}>
      <p>Its plane root no longer exists.</p>
    </Centered>
  );
}

/** A File tab with no diff shows the file itself. */
async function load(
  planeId: string,
  path: string,
  mode: FileTab["mode"],
  force: boolean,
): Promise<Content> {
  try {
    const diff = await commands.planeFileDiff(planeId, path, mode, force);
    if (diff.kind !== "plain") return { kind: "diff", diff };
    return { kind: "plain", text: await commands.planeReadFile(planeId, path) };
  } catch (error) {
    return { kind: "error", message: String(error) };
  }
}

/** `.md` / `.markdown`, any case. */
function isMarkdown(path: string): boolean {
  return /\.(md|markdown)$/i.test(path);
}

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}
