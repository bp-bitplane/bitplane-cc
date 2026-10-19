import { ChevronsDownUp, RefreshCw } from "lucide-react";
import { useEffect, useMemo } from "react";
import { labelFor } from "../../keymap";
import { findPlane } from "../../store/selectors";
import {
  loadSourceControl,
  openFile,
  openFileTab,
  refreshFiles,
  setFilesExpanded,
  useStore,
  watchFiles,
  watchSourceControl,
} from "../../store/store";
import { Centered } from "../../ui/Centered";
import { PanelActions, PanelButton } from "../PanelButton";
import { FileTree, type MenuAction, type MenuEntry } from "./FileTree";
import { fileStatuses, initialExpanded, listingKey, repoBranches, treeData } from "./filesTree";
import { useListings } from "./useListings";

const NONE: readonly string[] = [];

export function Files({ planeId }: { planeId: string }) {
  return <PlaneFiles key={planeId} planeId={planeId} />;
}

export function FilesActions({ planeId }: { planeId: string }) {
  const platform = useStore((s) => s.platform);
  return (
    <PanelActions>
      <PanelButton title="Collapse all" onClick={() => setFilesExpanded(planeId, [])}>
        <ChevronsDownUp size={14} strokeWidth={1.75} />
      </PanelButton>
      <PanelButton
        title={`Refresh (${labelFor(platform, "refresh")})`}
        onClick={() => {
          refreshFiles(planeId);
          void loadSourceControl(planeId);
        }}
      >
        <RefreshCw size={14} strokeWidth={1.75} />
      </PanelButton>
    </PanelActions>
  );
}

function PlaneFiles({ planeId }: { planeId: string }) {
  const planeDir = useStore((s) => findPlane(s, planeId)?.directory ?? null);
  const expanded = useStore((s) => s.filesExpanded[planeId]);
  const repos = useStore((s) =>
    s.sourceControl?.planeId === planeId ? s.sourceControl.repos : null,
  );
  const wanted = useMemo(() => (expanded ?? NONE).map(listingKey), [expanded]);
  const { listings, error, errors } = useListings(planeId, wanted);

  useEffect(() => {
    void loadSourceControl(planeId);
    return watchSourceControl(planeId);
  }, [planeId]);

  useEffect(() => {
    if (expanded === undefined && repos && planeDir) {
      setFilesExpanded(planeId, initialExpanded(repos, planeDir));
    }
  }, [planeId, planeDir, expanded, repos]);

  const watched = useMemo(
    () => ["", ...wanted.filter((dir) => listings.has(dir))].join("\0"),
    [wanted, listings],
  );
  useEffect(() => watchFiles(planeId, watched.split("\0")), [planeId, watched]);

  const tree = useMemo(() => treeData(listings, errors), [listings, errors]);
  const statuses = useMemo(
    () => fileStatuses(repos ?? [], planeDir ?? "", tree.ignored),
    [repos, planeDir, tree.ignored],
  );
  const decorations = useMemo(
    () => new Map([...repoBranches(repos ?? [], planeDir ?? ""), ...tree.errors]),
    [repos, planeDir, tree.errors],
  );

  if (error && !listings.has("")) {
    return (
      <Centered title="Couldn't list files">
        <p>{error}</p>
      </Centered>
    );
  }
  if (!listings.has("")) return <Centered title="Listing files…" />;
  return (
    <FileTree
      paths={tree.paths}
      folders={tree.folders}
      statuses={statuses}
      decorations={decorations}
      expanded={expanded ?? NONE}
      onExpandedChange={(paths) => setFilesExpanded(planeId, paths)}
      onOpen={(path) => {
        if (!tree.errors.has(path)) openFileTab(planeId, path, "file");
      }}
      onOpenExternal={(path) => {
        if (!tree.errors.has(path)) void openFile(planeId, path);
      }}
      menu={(path) => (tree.errors.has(path) ? [] : menuFor(planeId, planeDir, path))}
    />
  );
}

/** Folders only get the copy actions; files open as a File tab or in VS Code first. */
function menuFor(planeId: string, planeDir: string | null, path: string): MenuEntry[] {
  if (path.endsWith("/")) return copyActions(planeDir, path);
  return [
    { label: "Open", run: () => openFileTab(planeId, path, "file") },
    { label: "Open in VS Code", run: () => void openFile(planeId, path) },
    "separator",
    ...copyActions(planeDir, path),
  ];
}

function copyActions(planeDir: string | null, path: string): MenuAction[] {
  const relative = listingKey(path);
  const copy = (text: string) => () => void navigator.clipboard.writeText(text);
  return [
    ...(planeDir ? [{ label: "Copy path", run: copy(`${planeDir}/${relative}`) }] : []),
    { label: "Copy relative path", run: copy(relative) },
  ];
}
