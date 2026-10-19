import { X } from "lucide-react";
import type { ReactNode } from "react";
import { useShallow } from "zustand/react/shallow";
import { CreateProgress } from "../create-plane/CreateProgress";
import { FileTabView } from "../file-tab/FileTabView";
import type { AppStatus } from "../ipc";
import { labelFor } from "../keymap";
import { activeFileTab, activeSession, findPlane, tabsOf } from "../store/selectors";
import {
  dismissCreate,
  dismissNotice,
  openCreateDialog,
  openDialog,
  openSession,
  refresh,
  type State,
  useStore,
} from "../store/store";
import { fileTabKey } from "../tabs/fileTabs";
import { TabBar } from "../tabs/TabBar";
import { TerminalStack } from "../terminal/TerminalStack";
import { Centered } from "../ui/Centered";
import styles from "./MainArea.module.css";

/**
 * Tab bar, banners, and the active Tab (a terminal or a File tab) or a screen
 * in its place. Terminals stay mounted behind a File tab.
 */
export function MainArea() {
  const view = useStore(useShallow(mainView));
  const mac = useStore((s) => s.platform === "mac");
  const visibleId = useStore((s) =>
    view.screen === null ? (activeSession(s, s.selectedPlaneId)?.id ?? null) : null,
  );
  const fileTab = useStore((s) =>
    view.screen === null ? activeFileTab(s, s.selectedPlaneId) : undefined,
  );
  return (
    <main className={styles.main}>
      {view.tabs ? (
        <TabBar planeId={view.tabs} />
      ) : (
        mac && <div className={styles.dragStrip} data-tauri-drag-region />
      )}
      <Banners />
      <div className={styles.content}>
        <TerminalStack visibleId={visibleId} />
        {fileTab && (
          <div className={styles.screen}>
            <FileTabView key={`${fileTab.planeId}/${fileTabKey(fileTab)}`} tab={fileTab} />
          </div>
        )}
        {view.screen !== null && (
          <div className={styles.screen}>
            <Screen view={view} />
          </div>
        )}
      </div>
    </main>
  );
}

type Screen = "status" | "noPlanes" | "hint" | "gone" | "creating" | "noSessions";

/** Flat on purpose: every field is a primitive or a store-owned object, so `useShallow` can compare it. */
export interface MainView {
  /** The plane whose tab bar shows. */
  tabs: string | null;
  screen: Screen | null;
  planeId: string | null;
  status: AppStatus;
  planesDir: string | null;
  dirExists: boolean;
}

/** What the main area shows, by priority (spec §2, "Empty and transitional states"). */
export function mainView(state: State): MainView {
  const { status, planesDir, planesDirExists: dirExists } = state.planes;
  const planeId = state.selectedPlaneId;
  const view = (screen: Screen | null, tabs: string | null = null): MainView => ({
    tabs,
    screen,
    planeId,
    status,
    planesDir,
    dirExists,
  });
  if (status.kind !== "ready" && status.kind !== "loading") return view("status");
  if (planeId === null) {
    const empty = status.kind === "ready" && state.planes.planes.length === 0;
    return view(empty ? "noPlanes" : "hint");
  }
  const plane = findPlane(state, planeId);
  if (plane?.kind === "creating" || (!plane && state.creates[planeId])) return view("creating");
  if (tabsOf(state, planeId).length > 0) return view(null, planeId);
  return !plane || plane.kind === "gone" ? view("gone") : view("noSessions", planeId);
}

function Screen({ view }: { view: MainView }) {
  const platform = useStore((s) => s.platform);
  const createButton = (
    <button type="button" className="primary" onClick={openCreateDialog}>
      Create plane ({labelFor(platform, "createPlane")})
    </button>
  );
  const planeId = view.planeId ?? "";
  switch (view.screen) {
    case "status":
      return <StatusScreen status={view.status} />;
    case "noPlanes":
      return view.dirExists ? (
        <Centered title="No planes yet" actions={createButton}>
          <p>
            Nothing in {view.planesDir}. Create your first plane, or create one with bp in a
            terminal; it appears here automatically.
          </p>
        </Centered>
      ) : (
        <Centered title="Planes directory not found" actions={createButton}>
          <p>
            {view.planesDir} doesn't exist. Create a plane to create it, or set planes_dir in
            bitplane config.
          </p>
        </Centered>
      );
    case "hint":
      return (
        <Centered title="Select a plane">
          <p>
            Pick a plane in the sidebar, or press {labelFor(platform, "palette")} to switch to one.
          </p>
        </Centered>
      );
    case "gone":
      return (
        <Centered title={`${planeId} is gone`}>
          <p>Its plane root no longer exists and it has no running sessions.</p>
        </Centered>
      );
    case "creating":
      return <CreateProgress planeId={planeId} />;
    case "noSessions":
      return (
        <Centered
          title={`No sessions in ${planeId}`}
          actions={
            <button type="button" className="primary" onClick={() => void openSession(planeId)}>
              New session
            </button>
          }
        >
          <p>Press {labelFor(platform, "newTab")} to start a shell in the plane root.</p>
        </Centered>
      );
    default:
      return null;
  }
}

function StatusScreen({ status }: { status: AppStatus }) {
  const retry = (
    <button type="button" onClick={() => void refresh()}>
      Retry
    </button>
  );
  switch (status.kind) {
    case "bpNotFound":
      return (
        <Centered title="bp not found" actions={retry}>
          <p>
            bitplane-cc needs the bp CLI. Looked on your login shell's PATH. Install bp, then retry.
          </p>
        </Centered>
      );
    case "bpTooOld":
      return (
        <Centered title="bp is too old" actions={retry}>
          <p>
            Found bp {status.found}; bitplane-cc needs ≥ {status.required}. Upgrade bp, then retry.
          </p>
        </Centered>
      );
    case "bpIncompatible":
      return (
        <Centered
          title={
            status.version
              ? `bp ${status.version} is incompatible with this app`
              : "This bp is incompatible with this app"
          }
          actions={retry}
        >
          <pre className={styles.detail}>{status.error}</pre>
        </Centered>
      );
    case "gitMissing":
      return (
        <Centered title="git is missing or too old" actions={retry}>
          <p>{status.message}</p>
          <p>bp needs git 2.36 or newer.</p>
        </Centered>
      );
    case "listFailed":
      return (
        <Centered title="Couldn't list planes" actions={retry}>
          <p>{status.message}</p>
        </Centered>
      );
    default:
      return null;
  }
}

function Banners(): ReactNode[] {
  const planeId = useStore((s) => s.selectedPlaneId);
  const gone = useStore((s) => (planeId ? findPlane(s, planeId)?.kind === "gone" : false));
  const scriptFailed = useStore((s) => (planeId ? Boolean(s.scriptFailed[planeId]) : false));
  const notice = useStore((s) => s.notice);
  const banners: ReactNode[] = [];
  if (planeId && gone) {
    banners.push(
      <div key="gone" className={styles.danger} role="alert">
        <strong>{planeId}</strong>'s plane root no longer exists. Its sessions keep running; new
        sessions are disabled.
      </div>,
    );
  }
  if (planeId && scriptFailed) {
    banners.push(
      <div key="script" className={styles.warning} role="alert">
        <span>Post-create script failed.</span>
        <button
          type="button"
          className={styles.link}
          onClick={() => openDialog({ kind: "createLog", planeId })}
        >
          View log
        </button>
        <button
          type="button"
          className={styles.dismiss}
          aria-label="Dismiss"
          onClick={() => void dismissCreate(planeId)}
        >
          <X size={13} />
        </button>
      </div>,
    );
  }
  if (notice) {
    banners.push(
      <div key="notice" className={styles.danger} role="alert">
        <span>{notice}</span>
        <button
          type="button"
          className={styles.dismiss}
          aria-label="Dismiss"
          onClick={dismissNotice}
        >
          <X size={13} />
        </button>
      </div>,
    );
  }
  return banners;
}
