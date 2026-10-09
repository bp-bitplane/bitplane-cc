import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useStore } from "../store/store";
import { mockBackend, seed } from "../test/fixtures";
import { Dialogs } from "./Dialogs";

describe("quit dialog", () => {
  it("groups busy sessions by whether they resume", () => {
    mockBackend();
    seed({
      dialog: {
        kind: "quit",
        info: {
          busy: [
            { planeId: "alpha", processName: "claude", resumes: true, working: false },
            { planeId: "beta", processName: "claude", resumes: true, working: true },
            { planeId: "gamma", processName: "vim", resumes: false, working: false },
          ],
          creating: [],
        },
      },
    });
    render(<Dialogs />);
    const resume = within(screen.getByRole("region", { name: "Will resume" }));
    const lost = within(screen.getByRole("region", { name: "Will be lost" }));
    expect(resume.getAllByRole("listitem").map((li) => li.textContent)).toEqual(["alpha: claude"]);
    expect(lost.getAllByRole("listitem").map((li) => li.textContent)).toEqual([
      "beta: claude (mid-turn)",
      "gamma: vim",
    ]);
  });
});

describe("agent resume dialog", () => {
  it("shows the hooks and installs them on confirm", async () => {
    const backend = mockBackend();
    seed({
      dialog: {
        kind: "agentResume",
        hooks: {
          path: "/home/me/.claude/settings.json",
          status: "missing",
          entries: '{"Stop":[]}',
        },
        error: null,
      },
    });
    render(<Dialogs />);
    expect(screen.getByText('{"Stop":[]}')).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Add hooks" }));
    expect(backend.called("agent_hooks_install")).toHaveLength(1);
    expect(useStore.getState().dialog).toBeNull();
  });

  it("says when the hooks are already there", () => {
    mockBackend();
    seed({
      dialog: {
        kind: "agentResume",
        hooks: { path: "/s.json", status: "installed", entries: "{}" },
        error: null,
      },
    });
    render(<Dialogs />);
    expect(screen.getByText("Agent resume is already enabled.")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Add hooks" })).toBeNull();
  });
});
