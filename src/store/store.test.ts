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
import { activeSession, sessionsOf, tabLabel } from "./selectors";
import {
  applyCreate,
  applySession,
  createPlane,
  cyclePlane,
  cycleTab,
  goToPlane,
  goToTab,
  requestCloseSession,
  requestQuit,
  selectPlane,
  useStore,
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
    });
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
    const info = { busy: [{ planeId: "a", processName: "claude" }], creating: ["b"] };
    const backend = mockBackend({ quit_check: () => info });
    seed();
    await requestQuit();
    expect(backend.called("quit")).toHaveLength(0);
    expect(state().dialog).toEqual({ kind: "quit", info });
  });
});

it("zooms within bounds and resets to 13 px", () => {
  seed();
  zoom(1);
  zoom(1);
  expect(state().fontSize).toBe(15);
  for (let i = 0; i < 40; i++) zoom(-1);
  expect(state().fontSize).toBe(8);
  zoom(0);
  expect(state().fontSize).toBe(13);
});
