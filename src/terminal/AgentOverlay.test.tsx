import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
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
import { AgentOverlay } from "./AgentOverlay";

describe("AgentOverlay", () => {
  const claude = (status: ReturnType<typeof session>["status"]) =>
    session(1, "a", { agent: "claude", status });

  function show(tab = claude({ kind: "exited", code: 0 }), agents = {}) {
    seed({ planes: planesModel([plane("a")]), sessions: sessionsById(tab), agents });
    render(<AgentOverlay session={tab} visible />);
  }

  it("shows nothing while the agent runs", () => {
    mockBackend();
    show(claude({ kind: "running" }));
    expect(screen.queryByRole("alertdialog")).toBeNull();
  });

  it("says Claude exited, with OK focused and Start again without a link", () => {
    mockBackend();
    show();
    expect(screen.getByRole("alertdialog", { name: "Claude exited" })).toHaveTextContent(
      "Claude exited",
    );
    expect(screen.getByRole("button", { name: "OK" })).toHaveFocus();
    expect(screen.getByRole("button", { name: "Start again" })).toBeInTheDocument();
  });

  it("adds a non-zero exit code and offers Resume when the tab has a link", async () => {
    const backend = mockBackend();
    show(claude({ kind: "exited", code: 3 }), {
      a1: agent("a1", "key-1", { state: { kind: "exited" } }),
    });
    expect(screen.getByText("Claude exited (code 3)")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Resume" }));
    await flush();
    expect(backend.called("session_restart")[0]?.args).toEqual({ id: 1 });
  });

  it("closes the tab on Enter or Escape", async () => {
    const backend = mockBackend();
    show();
    await userEvent.keyboard("{Enter}");
    await flush();
    expect(backend.called("session_close")).toHaveLength(1);
    await userEvent.keyboard("{Escape}");
    await flush();
    expect(backend.called("session_close")).toHaveLength(2);
  });

  it("shows why it couldn't start, with Retry", async () => {
    const backend = mockBackend();
    show(claude({ kind: "spawnFailed", error: "claude was not found on PATH" }));
    expect(screen.getByText("Claude couldn't start")).toBeInTheDocument();
    expect(screen.getByText("claude was not found on PATH")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "OK" })).toHaveFocus();
    await userEvent.click(screen.getByRole("button", { name: "Retry" }));
    await flush();
    expect(backend.called("session_restart")).toHaveLength(1);
  });
});
