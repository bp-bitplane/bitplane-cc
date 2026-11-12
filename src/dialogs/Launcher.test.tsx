import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MainArea } from "../main-area/MainArea";
import { selectPlane, useStore } from "../store/store";
import { agent, flush, mockBackend, plane, planesModel, seed, session } from "../test/fixtures";
import { Dialogs } from "./Dialogs";

vi.mock("../terminal/TerminalStack", () => ({ TerminalStack: () => null }));

const options = () => screen.getAllByRole("option");
const labels = () => options().map((o) => o.textContent);
const selected = () => screen.getByRole("option", { selected: true }).textContent;

describe("Launcher", () => {
  beforeEach(() => {
    mockBackend();
    seed({
      planes: planesModel([plane("kiosk"), plane("docs")]),
      agentKinds: [
        { kind: "claude", available: true, error: null },
        { kind: "codex", available: false, error: "codex not found on PATH" },
      ],
      agents: {
        here: agent("here", null, { state: { kind: "exited" }, cwd: "/planes/kiosk/api" }),
        live: agent("live", "key-1", { cwd: "/planes/kiosk" }),
        other: agent("other", null, { state: { kind: "exited" }, cwd: "/planes/docs" }),
      },
      selectedPlaneId: "kiosk",
      dialog: { kind: "launcher", planeId: "kiosk" },
    });
  });

  it("lists Terminal, then the kinds, then this plane's resumable sessions, Terminal selected", () => {
    render(<Dialogs />);
    expect(labels()).toEqual(["Terminal", "Claude", "codexcodex not found on PATH", "Resume here"]);
    expect(selected()).toBe("Terminal");
  });

  it("opens a terminal on Enter", async () => {
    const backend = mockBackend({ session_open: () => session(5, "kiosk") });
    render(<Dialogs />);
    await userEvent.keyboard("{Enter}");
    await flush();
    expect(backend.called("session_open")[0]?.args).toEqual({ planeId: "kiosk", agent: null });
    expect(useStore.getState().activeTab.kiosk).toBe(5);
    expect(useStore.getState().dialog).toBeNull();
  });

  it("opens an agent terminal of the picked kind", async () => {
    const backend = mockBackend({ session_open: () => session(6, "kiosk", { agent: "claude" }) });
    render(<Dialogs />);
    await userEvent.keyboard("{ArrowDown}{Enter}");
    await flush();
    expect(backend.called("session_open")[0]?.args).toEqual({
      planeId: "kiosk",
      agent: { kind: "claude", resume: null, cwd: null },
    });
  });

  it("greys out an unavailable kind with its error and never starts it", async () => {
    const backend = mockBackend();
    render(<Dialogs />);
    const codex = screen.getByRole("option", { name: /codex/ });
    expect(codex).toHaveAttribute("aria-disabled", "true");
    expect(codex).toHaveAttribute("title", "codex not found on PATH");
    await userEvent.click(codex);
    expect(backend.called("session_open")).toHaveLength(0);
    await userEvent.keyboard("{ArrowDown}{ArrowDown}");
    expect(selected()).toBe("Resume here");
  });

  it("follows a kind's availability live", () => {
    render(<Dialogs />);
    act(() =>
      useStore.setState({
        agentKinds: [
          { kind: "claude", available: false, error: "claude not found on PATH" },
          { kind: "codex", available: true, error: null },
        ],
      }),
    );
    expect(screen.getByRole("option", { name: /Claude/ })).toHaveAttribute("aria-disabled", "true");
    expect(screen.getByRole("option", { name: /codex/ })).toHaveAttribute("aria-disabled", "false");
  });

  it("resumes an agent session of this plane in its cwd", async () => {
    const backend = mockBackend({ session_open: () => session(7, "kiosk") });
    render(<Dialogs />);
    await userEvent.keyboard("resume{Enter}");
    await flush();
    expect(backend.called("session_open")[0]?.args).toEqual({
      planeId: "kiosk",
      agent: { kind: "claude", resume: "here", cwd: "/planes/kiosk/api" },
    });
  });

  it("selects the first match while filtering", async () => {
    render(<Dialogs />);
    await userEvent.keyboard("cl");
    expect(labels()).toEqual(["Claude"]);
    expect(selected()).toBe("Claude");
  });
});

describe("an empty plane", () => {
  it("opens the launcher; Esc leaves the empty-plane view, whose New session reopens it", async () => {
    mockBackend();
    seed({ planes: planesModel([plane("kiosk")]) });
    render(
      <>
        <MainArea />
        <Dialogs />
      </>,
    );
    act(() => selectPlane("kiosk"));
    expect(screen.getByRole("dialog", { name: "Start in kiosk" })).toBeInTheDocument();
    await userEvent.keyboard("{Escape}");
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(screen.getByText("No sessions in kiosk")).toBeInTheDocument();
    await userEvent.click(screen.getByText("New session", { selector: "button" }));
    expect(screen.getByRole("dialog", { name: "Start in kiosk" })).toBeInTheDocument();
  });
});
