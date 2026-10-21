import { Code2, ExternalLink, Eye } from "lucide-react";
import { lazy, type ReactNode, Suspense, useCallback, useEffect, useState } from "react";
import { commands, type FileContents, type FileDiff, type FileText } from "../ipc";
import { findPlane } from "../store/selectors";
import { type DiffStyle, onFileTabsSignal, openFile, setDiffStyle, useStore } from "../store/store";
import { type FileTab, fileTabTitle } from "../tabs/fileTabs";
import { Centered } from "../ui/Centered";
import styles from "./FileTabView.module.css";
import { changedLines, firstChangedLine, MAX_CHANGED_LINES, renameOf } from "./patch";
import { isMarkdown, type MdVariant, useMdVariant } from "./prototype/mdVariant";

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
  /** Bumped when git or the file changed; the last content stays up meanwhile. */
  const [version, setVersion] = useState(0);
  const available = planeKind !== "gone" && planeKind !== "creating";
  // PROTOTYPE (#37): the Markdown preview, per variant (see prototype/mdVariant.ts).
  const variant = useMdVariant();
  /** `null` until the user toggles: the variant's default applies. */
  const [previewChoice, setPreviewChoice] = useState<boolean | null>(null);
  const [newSide, setNewSide] = useState<string | null>(null);
  useEffect(() => {
    void variant;
    setPreviewChoice(null);
  }, [variant]);

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

  const patch = content.kind === "diff" && content.diff.kind === "diff" ? content.diff.patch : null;
  const preview = previewState(variant, path, content, patch, previewChoice);
  const previewDiff = preview.on && patch !== null;

  useEffect(() => {
    if (!previewDiff) return;
    void version;
    let current = true;
    void loadContents().then(
      (contents) => current && setNewSide(contents.new),
      (error) => current && setNewSide(`Couldn't read the file: ${String(error)}`),
    );
    return () => {
      current = false;
    };
  }, [previewDiff, loadContents, version]);

  if (!available) return <PlaneNotice planeId={planeId} creating={planeKind === "creating"} />;

  const rename = patch ? renameOf(patch) : null;
  const line = patch ? firstChangedLine(patch) : undefined;
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
        {variant === "A" && preview.offered && !patch && (
          <fieldset className={styles.toggle} aria-label="Markdown view">
            <button
              type="button"
              aria-pressed={!preview.on}
              onClick={() => setPreviewChoice(false)}
            >
              Source
            </button>
            <button type="button" aria-pressed={preview.on} onClick={() => setPreviewChoice(true)}>
              Preview
            </button>
          </fieldset>
        )}
        {variant === "C" && preview.offered && (
          <fieldset className={styles.toggle} aria-label="View">
            {patch ? (
              (["unified", "split"] as const).map((style) => (
                <button
                  key={style}
                  type="button"
                  aria-pressed={!preview.on && diffStyle === style}
                  onClick={() => {
                    setPreviewChoice(false);
                    setDiffStyle(style);
                  }}
                >
                  {style === "unified" ? "Unified" : "Split"}
                </button>
              ))
            ) : (
              <button
                type="button"
                aria-pressed={!preview.on}
                onClick={() => setPreviewChoice(false)}
              >
                Source
              </button>
            )}
            <button type="button" aria-pressed={preview.on} onClick={() => setPreviewChoice(true)}>
              Preview
            </button>
          </fieldset>
        )}
        {patch && !(variant === "C" && preview.offered) && (
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
        {variant === "B" && isMarkdown(path) && (patch || preview.offered) && (
          <button
            type="button"
            className={styles.icon}
            aria-pressed={preview.on}
            disabled={!preview.offered}
            title={
              !preview.offered
                ? "Preview is only for the file itself, not its diff"
                : preview.on
                  ? "Show source"
                  : "Show preview"
            }
            aria-label={preview.on ? "Show source" : "Show preview"}
            onClick={() => setPreviewChoice(!preview.on)}
          >
            {preview.on ? <Code2 size={14} /> : <Eye size={14} />}
          </button>
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
        {preview.on ? (
          <Suspense fallback={null}>
            {previewDiff
              ? newSide !== null && (
                  <MarkdownView
                    source={newSide}
                    note={`Previewing the ${tab.mode === "index" ? "staged" : "working tree"} version, not the changes. Switch to Unified or Split for the diff.`}
                  />
                )
              : content.kind === "plain" &&
                content.text.kind === "text" && <MarkdownView source={content.text.contents} />}
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

/**
 * PROTOTYPE (#37): whether this tab offers a Markdown preview and shows it.
 * A: Source by default, only for the plain file. B: Preview by default, only
 * for the plain file (its toggle shows disabled on a diff). C: Preview by
 * default for the plain file, and offered on a diff too (renders the new side).
 */
function previewState(
  variant: MdVariant,
  path: string,
  content: Content,
  patch: string | null,
  choice: boolean | null,
): { offered: boolean; on: boolean } {
  if (!isMarkdown(path)) return { offered: false, on: false };
  const plainText = content.kind === "plain" && content.text.kind === "text";
  const offered = plainText || (variant === "C" && patch !== null);
  const byDefault = variant !== "A" && plainText;
  return { offered, on: offered && (choice ?? byDefault) };
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

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}
