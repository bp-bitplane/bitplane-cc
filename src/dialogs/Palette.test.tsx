import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useStore } from "../store/store";
import {
  agent,
  flush,
  mockBackend,
  plane,
  planesModel,
  seed,
  session,
  sessionsById,
} from "../test/fixtures";
import { Palette } from "./Palette";

describe("Palette", () => {
  beforeEach(() => {
    mockBackend();
    seed({
      planes: planesModel([plane("acme-billing"), plane("docs"), plane("kiosk", "plane", ["x"])]),
      sessions: sessionsById(session(1, "acme-billing"), session(2, "acme-billing")),
      dialog: { kind: "palette" },
    });
  });

  it("filters by substring and selects with the keyboard", async () => {
    render(<Palette />);
    await userEvent.keyboard("i");
    expect(screen.getAllByRole("option").map((o) => o.firstChild?.textContent)).toEqual([
      "acme-billing",
      "kiosk",
    ]);
    await userEvent.keyboard("{ArrowDown}{Enter}");
    expect(useStore.getState().selectedPlaneId).toBe("kiosk");
    expect(useStore.getState().dialog).toEqual({ kind: "launcher", planeId: "kiosk" });
  });

  it("shows session counts and findings", () => {
    render(<Palette />);
    expect(screen.getByTitle("2 open sessions")).toBeInTheDocument();
    expect(screen.getByRole("img", { name: "findings" })).toBeInTheDocument();
  });

  it("resumes an agent session that exited, in the plane it ran in", async () => {
    const backend = mockBackend({ session_open: () => session(5, "kiosk") });
    useStore.setState({
      agents: {
        gone: agent("gone", null, { state: { kind: "exited" }, cwd: "/planes/kiosk/api" }),
        live: agent("live", "key-1", { cwd: "/planes/kiosk" }),
        elsewhere: agent("elsewhere", null, { state: { kind: "exited" }, cwd: "/tmp" }),
      },
    });
    render(<Palette />);
    await userEvent.keyboard("resume");
    expect(screen.getAllByRole("option").map((o) => o.textContent)).toEqual([
      "Resume agent session gone in kiosk",
    ]);
    await userEvent.keyboard("{Enter}");
    await flush();
    expect(backend.called("session_open")[0]?.args).toEqual({
      planeId: "kiosk",
      agent: { kind: "claude", resume: "gone", cwd: "/planes/kiosk/api" },
    });
    expect(useStore.getState().activeTab.kiosk).toBe(5);
    expect(useStore.getState().dialog).toBeNull();
  });

  it("says when nothing matches and closes on Escape", async () => {
    render(<Palette />);
    await userEvent.keyboard("zzz");
    expect(screen.getByText("No plane matches")).toBeInTheDocument();
    await userEvent.keyboard("{Escape}");
    expect(useStore.getState().dialog).toBeNull();
  });
});
