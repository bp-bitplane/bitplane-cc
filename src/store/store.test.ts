import {
  createInfo,
  flush,
  mockBackend,
  plane,
  planesModel,
  seed,
  session,
  sessionsById,
} from "../test/fixtures";
import { activeFileTab, activeSession, fileTabsOf, sessionsOf, tabLabel } from "./selectors";
import {
  applyCreate,
  applySession,
  closeActiveTab,
  closeFileTab,
  createPlane,
  cyclePlane,
  cycleTab,
  followGitWatch,
  goToPlane,
  goToTab,
  loadSnapshot,
  loadUiState,
  openFileTab,
  pinFileTab,
  requestCloseSession,
  requestQuit,
  selectPlane,
  selectSession,
  useStore,
  watchSourceControl,
  zoom,
} from "./store";

const state = () => useStore.getState();

describe("selecting planes", () => {
  it("opens a session when the plane has none", async () => {
    const backend = mockBackend({ session_open: () => session(7, "alpha") });
    seed({ planes: planesModel([plane("alpha")]) });
    selectPlane("alpha");
    await flush();
    expect(backend.called("session_open")[0]?.args).toEqual({ planeId: "alpha" });
    expect(state().selectedPlaneId).toBe("alpha");
    expect(state().activeTab.alpha).toBe(7);
    expect(state().sessions[7]).toBeDefined();
  });

  it("opens a session in a draft plane too", async () => {
    const backend = mockBackend({ session_open: () => session(1, "scratch") });
    seed({ planes: planesModel([plane("scratch", "draft")]) });
    selectPlane("scratch");
    await flush();
    expect(backend.called("session_open")).toHaveLength(1);
  });

  it.each(["gone", "creating"] as const)("never opens a session in a %s plane", async (kind) => {
    const backend = mockBackend();
    seed({ planes: planesModel([plane("p", kind)]) });
    selectPlane("p");
    await flush();
    expect(backend.called("session_open")).toHaveLength(0);
  });

  it("keeps existing sessions instead of opening another", async () => {
    const backend = mockBackend();
    seed({ planes: planesModel([plane("alpha")]), sessions: sessionsById(session(1, "alpha")) });
    selectPlane("alpha");
    await flush();
    expect(backend.called("session_open")).toHaveLength(0);
  });

  it("shows the error when the root is missing", async () => {
    mockBackend({
      session_open: () => {
        throw "the plane root /planes/alpha no longer exists";
      },
    });
    seed({ planes: planesModel([plane("alpha")]) });
    selectPlane("alpha");
    await flush();
    expect(state().notice).toContain("no longer exists");
  });

  it("cycles and jumps through the sidebar rows", () => {
    mockBackend();
    const rows = [plane("a"), plane("b"), plane("c")];
    seed({
      planes: planesModel(rows),
      sessions: sessionsById(session(1, "a"), session(2, "b"), session(3, "c")),
    });
    cyclePlane(1);
    expect(state().selectedPlaneId).toBe("a");
    cyclePlane(-1);
    expect(state().selectedPlaneId).toBe("c");
    goToPlane(2);
    expect(state().selectedPlaneId).toBe("b");
    goToPlane(9);
    expect(state().selectedPlaneId).toBe("b");
  });
});

describe("event ordering", () => {
  it("doesn't revive a session that closed before session_open replied", async () => {
    mockBackend({
      session_open: () => {
        applySession({ id: 21, session: session(21, "alpha") });
        applySession({ id: 21, session: null });
        return session(21, "alpha");
      },
    });
    seed({ planes: planesModel([plane("alpha")]) });
    selectPlane("alpha");
    await flush();
    expect(state().sessions[21]).toBeUndefined();
  });

  it("keeps a newer status than the session_open reply", async () => {
    const exited = session(22, "alpha", { status: { kind: "exited", code: 2 } });
    mockBackend({
      session_open: () => {
        applySession({ id: 22, session: exited });
        return session(22, "alpha");
      },
    });
    seed({ planes: planesModel([plane("alpha")]) });
    selectPlane("alpha");
    await flush();
    expect(state().sessions[22]).toEqual(exited);
  });
});

describe("creating a plane", () => {
  it("selects the plane without opening a shell in it", async () => {
    const backend = mockBackend();
    seed({ planes: planesModel([]) });
    await createPlane(createInfo("feat").args);
    await flush();
    expect(state().selectedPlaneId).toBe("feat");
    expect(backend.called("session_open")).toHaveLength(0);
  });
});

describe("tabs", () => {
  beforeEach(() => {
    mockBackend();
    seed({
      planes: planesModel([plane("a")]),
      sessions: sessionsById(session(1, "a"), session(5, "a"), session(9, "a")),
      selectedPlaneId: "a",
      activeTab: { a: 5 },
    });
  });

  it("are numbered by position", () => {
    const labels = sessionsOf(state(), "a").map((s, i) => tabLabel(s, i + 1));
    expect(labels).toEqual(["1 · zsh", "2 · zsh", "3 · zsh"]);
  });

  it("cycle with wrap-around and jump by position", () => {
    cycleTab(1);
    expect(activeSession(state(), "a")?.id).toBe(9);
    cycleTab(1);
    expect(activeSession(state(), "a")?.id).toBe(1);
    goToTab(3);
    expect(activeSession(state(), "a")?.id).toBe(9);
  });

  it("hand focus to the neighbour when the active one closes", () => {
    applySession({ id: 5, session: null });
    expect(state().activeTab.a).toBe(9);
    expect(sessionsOf(state(), "a").map((s) => s.id)).toEqual([1, 9]);
    applySession({ id: 9, session: null });
    expect(state().activeTab.a).toBe(1);
    applySession({ id: 1, session: null });
    expect(state().activeTab.a).toBeUndefined();
  });
});

describe("File tabs", () => {
  beforeEach(() => {
    mockBackend();
    seed({
      planes: planesModel([plane("a"), plane("b")]),
      sessions: sessionsById(session(1, "a"), session(2, "a")),
      selectedPlaneId: "b",
      activeTab: { a: 1 },
    });
  });

  const keys = () =>
    fileTabsOf(state(), "a").map((t) => `${t.mode}:${t.path}${t.pinned ? "!" : ""}`);

  it("open as the active Tab of their plane and select it", () => {
    openFileTab("a", "src/foo.ts", "worktree");
    expect(state().selectedPlaneId).toBe("a");
    expect(state().activeTab.a).toBe("worktree:src/foo.ts");
    expect(activeSession(state(), "a")).toBeUndefined();
    expect(activeFileTab(state(), "a")).toEqual({
      planeId: "a",
      path: "src/foo.ts",
      mode: "worktree",
      pinned: false,
    });
  });

  it("focus an open one instead of opening it twice", () => {
    openFileTab("a", "src/foo.ts", "index");
    pinFileTab("a", "index:src/foo.ts");
    openFileTab("a", "src/bar.ts", "file");
    openFileTab("a", "src/foo.ts", "index");
    expect(keys()).toEqual(["index:src/foo.ts!", "file:src/bar.ts"]);
    expect(state().activeTab.a).toBe("index:src/foo.ts");
  });

  it("tell modes of one path apart", () => {
    openFileTab("a", "src/foo.ts", "index");
    pinFileTab("a", "index:src/foo.ts");
    openFileTab("a", "src/foo.ts", "worktree");
    expect(keys()).toEqual(["index:src/foo.ts!", "worktree:src/foo.ts"]);
  });

  it("replace the preview in place until pinned", () => {
    openFileTab("a", "one.ts", "file");
    openFileTab("a", "two.ts", "file");
    expect(keys()).toEqual(["file:two.ts"]);
    pinFileTab("a", "file:two.ts");
    openFileTab("a", "three.ts", "file");
    expect(keys()).toEqual(["file:two.ts!", "file:three.ts"]);
  });

  it("keep one preview per plane", () => {
    openFileTab("a", "one.ts", "file");
    openFileTab("b", "two.ts", "file");
    expect(keys()).toEqual(["file:one.ts"]);
    expect(fileTabsOf(state(), "b").map((t) => t.path)).toEqual(["two.ts"]);
  });

  it("cycle after the Session tabs", () => {
    openFileTab("a", "one.ts", "file");
    cycleTab(1);
    expect(activeSession(state(), "a")?.id).toBe(1);
    cycleTab(-1);
    expect(state().activeTab.a).toBe("file:one.ts");
    cycleTab(-1);
    expect(state().activeTab.a).toBe(2);
  });

  it("leave ⌘1…9 to sessions", () => {
    openFileTab("a", "one.ts", "file");
    goToTab(3);
    expect(state().activeTab.a).toBe("file:one.ts");
    goToTab(2);
    expect(state().activeTab.a).toBe(2);
  });

  it("close and hand focus to the neighbour", () => {
    openFileTab("a", "one.ts", "file");
    pinFileTab("a", "file:one.ts");
    openFileTab("a", "two.ts", "file");
    closeFileTab("a", "file:two.ts");
    expect(state().activeTab.a).toBe("file:one.ts");
    closeActiveTab();
    expect(keys()).toEqual([]);
    expect(state().activeTab.a).toBe(2);
  });

  it("take focus when the last session closes", () => {
    openFileTab("a", "one.ts", "file");
    selectSession("a", 1);
    applySession({ id: 1, session: null });
    applySession({ id: 2, session: null });
    expect(state().activeTab.a).toBe("file:one.ts");
  });

  it("let ⌘W close the active session too", async () => {
    const backend = mockBackend({ session_busy: () => ({ busy: false, processName: null }) });
    openFileTab("a", "one.ts", "file");
    selectSession("a", 2);
    closeActiveTab();
    await flush();
    expect(backend.called("session_close")[0]?.args).toEqual({ id: 2 });
    expect(keys()).toEqual(["file:one.ts"]);
  });
});

describe("the git watch", () => {
  const targets = (backend: ReturnType<typeof mockBackend>) =>
    backend.called("plane_git_watch").map((c) => c.args.target);

  it("follows the shown plane's File tabs and Source control", () => {
    const backend = mockBackend();
    seed({ planes: planesModel([plane("a"), plane("b")]), selectedPlaneId: "a" });
    const stop = followGitWatch();
    openFileTab("a", "web/x.ts", "worktree");
    pinFileTab("a", "worktree:web/x.ts");
    openFileTab("a", "web/x.ts", "index");
    openFileTab("a", "plane.toml", "file");
    const stopSourceControl = watchSourceControl("a");
    stopSourceControl();
    closeFileTab("a", "worktree:web/x.ts");
    closeFileTab("a", "file:plane.toml");
    stop();
    expect(targets(backend)).toEqual([
      { planeId: "a", sourceControl: false, fileTabs: ["web/x.ts"] },
      { planeId: "a", sourceControl: false, fileTabs: ["web/x.ts", "plane.toml"] },
      { planeId: "a", sourceControl: true, fileTabs: ["web/x.ts", "plane.toml"] },
      { planeId: "a", sourceControl: false, fileTabs: ["web/x.ts", "plane.toml"] },
      { planeId: "a", sourceControl: false, fileTabs: ["plane.toml"] },
      null,
    ]);
  });

  it("drops another plane's File tabs when the selection moves", () => {
    const backend = mockBackend();
    seed({ planes: planesModel([plane("a"), plane("b")]), selectedPlaneId: "a" });
    const stop = followGitWatch();
    openFileTab("a", "x.ts", "file");
    useStore.setState({ selectedPlaneId: "b" });
    stop();
    expect(targets(backend)).toEqual([
      { planeId: "a", sourceControl: false, fileTabs: ["x.ts"] },
      null,
    ]);
  });
});

describe("closing sessions", () => {
  it("closes an idle shell straight away", async () => {
    const backend = mockBackend({ session_busy: () => ({ busy: false, processName: null }) });
    seed({ sessions: sessionsById(session(1, "a")), selectedPlaneId: "a" });
    await requestCloseSession();
    expect(backend.called("session_close")[0]?.args).toEqual({ id: 1 });
  });

  it("asks first when a program is running", async () => {
    const backend = mockBackend({ session_busy: () => ({ busy: true, processName: "vim" }) });
    seed({ sessions: sessionsById(session(1, "a")), selectedPlaneId: "a" });
    await requestCloseSession(1);
    expect(backend.called("session_close")).toHaveLength(0);
    expect(state().dialog).toEqual({ kind: "confirmClose", sessionId: 1, processName: "vim" });
  });

  it("doesn't ask for an exited session", async () => {
    const backend = mockBackend();
    seed({ sessions: sessionsById(session(1, "a", { status: { kind: "exited", code: 1 } })) });
    await requestCloseSession(1);
    expect(backend.called("session_busy")).toHaveLength(0);
    expect(backend.called("session_close")).toHaveLength(1);
  });
});

describe("create outcomes", () => {
  it("selects the new plane and opens a session on success", async () => {
    const backend = mockBackend({ session_open: () => session(3, "feat") });
    seed({ creates: { feat: createInfo("feat") } });
    applyCreate({ id: "feat", create: createInfo("feat", { status: { kind: "succeeded" } }) });
    await flush();
    expect(backend.called("plane_create_dismiss")[0]?.args).toEqual({ id: "feat" });
    expect(state().selectedPlaneId).toBe("feat");
    expect(backend.called("session_open")).toHaveLength(1);
  });

  it("keeps the log and shows the banner when a script failed", async () => {
    const backend = mockBackend({ session_open: () => session(3, "feat") });
    seed({ creates: { feat: createInfo("feat") } });
    applyCreate({ id: "feat", create: createInfo("feat", { status: { kind: "scriptFailed" } }) });
    await flush();
    expect(state().scriptFailed.feat).toBe(true);
    expect(backend.called("plane_create_dismiss")).toHaveLength(0);
    expect(backend.called("session_open")).toHaveLength(1);
  });

  it("reopens the dialog with the entries when nothing was created", async () => {
    mockBackend();
    const envelope = {
      error: "create_aborted",
      code: 1,
      message: "a worktree failed",
      problems: [],
      remedy: null,
    };
    seed({ creates: { feat: createInfo("feat") }, selectedPlaneId: "feat" });
    applyCreate({
      id: "feat",
      create: createInfo("feat", {
        status: { kind: "nothingCreated", envelope, retryable: false },
        log: "git said no",
      }),
    });
    await flush();
    expect(state().selectedPlaneId).toBeNull();
    expect(state().dialog).toEqual({
      kind: "create",
      initial: createInfo("feat").args,
      failure: { envelope, log: "git said no", retryable: false },
      duplicateOf: null,
    });
  });

  it("reopens a failed duplicate still duplicating its source", async () => {
    mockBackend();
    seed({ planes: planesModel([]) });
    await createPlane(createInfo("feat").args, "alpha");
    applyCreate({ id: "feat", create: createInfo("feat") });
    applyCreate({
      id: "feat",
      create: createInfo("feat", {
        status: { kind: "nothingCreated", envelope: null, retryable: true },
      }),
    });
    await flush();
    expect(state().dialog).toMatchObject({ kind: "create", duplicateOf: "alpha" });
    expect(state().duplicates).toEqual({});
  });

  it("reacts only to the transition out of running", async () => {
    const backend = mockBackend();
    seed({ creates: { feat: createInfo("feat", { status: { kind: "succeeded" } }) } });
    applyCreate({ id: "feat", create: createInfo("feat", { status: { kind: "succeeded" } }) });
    await flush();
    expect(backend.calls).toHaveLength(0);
  });
});

describe("quitting", () => {
  it("quits without asking when nothing is running", async () => {
    const backend = mockBackend({ quit_check: () => ({ busy: [], creating: [] }) });
    seed();
    await requestQuit();
    expect(backend.called("quit")).toHaveLength(1);
  });

  it("asks when sessions are busy or planes are being created", async () => {
    const info = {
      busy: [{ planeId: "a", processName: "vim", resumes: false, working: false }],
      creating: ["b"],
    };
    const backend = mockBackend({ quit_check: () => info });
    seed();
    await requestQuit();
    expect(backend.called("quit")).toHaveLength(0);
    expect(state().dialog).toEqual({ kind: "quit", info });
  });

  it("quits without asking when every busy session resumes", async () => {
    const info = {
      busy: [{ planeId: "a", processName: "claude", resumes: true, working: false }],
      creating: [],
    };
    const backend = mockBackend({ quit_check: () => info });
    seed();
    await requestQuit();
    expect(backend.called("quit")).toHaveLength(1);
  });

  it("asks when an agent is mid-turn", async () => {
    const info = {
      busy: [{ planeId: "a", processName: "claude", resumes: true, working: true }],
      creating: [],
    };
    const backend = mockBackend({ quit_check: () => info });
    seed();
    await requestQuit();
    expect(backend.called("quit")).toHaveLength(0);
  });
});

describe("restoring", () => {
  it("says which planes were skipped", () => {
    seed();
    loadSnapshot({
      planes: planesModel([]),
      sessions: [],
      creates: [],
      restoreSkipped: ["a", "b"],
    });
    expect(state().notice).toBe("Couldn't restore the sessions of a, b: the planes are gone.");
  });

  it("maps saved tabs back to the restored sessions by key", () => {
    seed({
      sessions: sessionsById(session(1, "alpha"), session(2, "alpha", { key: "k-two" })),
      activeTab: { alpha: 1 },
    });
    loadUiState(
      JSON.stringify({
        selectedPlaneId: "alpha",
        activeTab: { alpha: "k-two", beta: "k-unknown" },
        collapsed: { beta: true },
        fontSize: 99,
      }),
    );
    expect(state().selectedPlaneId).toBe("alpha");
    expect(state().activeTab).toEqual({ alpha: 2 });
    expect(state().collapsed).toEqual({ beta: true });
    expect(state().fontSize).toBe(32);
  });

  it("ignores a missing or garbled UI state", () => {
    seed({ activeTab: { alpha: 1 }, fontSize: 14 });
    loadUiState(null);
    loadUiState("{ torn");
    loadUiState("[]");
    expect(state().activeTab).toEqual({ alpha: 1 });
    expect(state().fontSize).toBe(14);
  });
});

it("zooms within bounds and resets to 12 px", () => {
  seed();
  zoom(1);
  zoom(1);
  expect(state().fontSize).toBe(14);
  for (let i = 0; i < 40; i++) zoom(-1);
  expect(state().fontSize).toBe(8);
  zoom(0);
  expect(state().fontSize).toBe(12);
});
