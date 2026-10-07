import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useStore } from "../store/store";
import {
  createInfo,
  flush,
  mockBackend,
  plane,
  planesModel,
  seed,
  session,
} from "../test/fixtures";
import { CreatePlaneDialog } from "./CreatePlaneDialog";

const projects = [
  { name: "sos/api", sourceRepoPresent: true },
  { name: "sos/web", sourceRepoPresent: true },
  { name: "lost", sourceRepoPresent: false },
];

describe("CreatePlaneDialog", () => {
  beforeEach(() => {
    seed({
      planes: planesModel([plane("taken")]),
      dialog: { kind: "create", initial: null, failure: null },
    });
  });

  describe("Empty", () => {
    it("makes a draft plane, selects it and opens a session", async () => {
      const backend = mockBackend({ session_open: () => session(1, "scratch") });
      render(<CreatePlaneDialog initial={null} failure={null} />);
      await userEvent.type(screen.getByRole("textbox", { name: /Id/ }), "scratch");
      await userEvent.click(screen.getByRole("button", { name: "Create" }));
      await flush();
      expect(backend.called("plane_create_draft")[0]?.args).toEqual({ id: "scratch" });
      expect(useStore.getState().selectedPlaneId).toBe("scratch");
      expect(useStore.getState().dialog).toBeNull();
      expect(backend.called("session_open")).toHaveLength(1);
    });

    it("validates the id as you type", async () => {
      mockBackend();
      render(<CreatePlaneDialog initial={null} failure={null} />);
      const id = screen.getByRole("textbox", { name: /Id/ });
      await userEvent.type(id, "taken");
      expect(screen.getByText("taken already exists")).toBeInTheDocument();
      await userEvent.clear(id);
      await userEvent.type(id, "bp-x");
      expect(screen.getByText(/reserved/)).toBeInTheDocument();
      expect(screen.getByRole("button", { name: "Create" })).toBeDisabled();
    });
  });

  describe("From projects", () => {
    it("builds every bp create option", async () => {
      const backend = mockBackend({ project_list: () => projects });
      render(<CreatePlaneDialog initial={null} failure={null} />);
      await userEvent.click(screen.getByRole("tab", { name: "From projects" }));

      await userEvent.type(screen.getByRole("textbox", { name: "Branch" }), "feat/Login");
      expect(screen.getByRole("textbox", { name: /^Id/ })).toHaveValue("feat-login");

      await userEvent.click(await screen.findByRole("checkbox", { name: "sos/api" }));
      await userEvent.click(screen.getByRole("checkbox", { name: "sos/web" }));
      expect(screen.getByRole("checkbox", { name: "lost" })).toBeDisabled();
      await userEvent.type(screen.getByRole("textbox", { name: "Branch for @sos/web" }), "main");

      await userEvent.click(screen.getByRole("radio", { name: "New" }));
      await userEvent.click(screen.getByText("Advanced"));
      await userEvent.click(screen.getByRole("checkbox", { name: "Skip fetch" }));
      await userEvent.click(screen.getByRole("checkbox", { name: "Skip post-create scripts" }));
      await userEvent.click(screen.getByRole("button", { name: "Create plane" }));
      await flush();

      expect(backend.called("plane_create")[0]?.args).toEqual({
        args: {
          id: "feat-login",
          branch: "feat/Login",
          members: [
            { spec: "@sos/api", branch: null },
            { spec: "@sos/web", branch: "main" },
          ],
          intent: "new",
          noFetch: true,
          noScripts: true,
        },
      });
      expect(useStore.getState().selectedPlaneId).toBe("feat-login");
    });

    it("keeps an edited id when the branch changes", async () => {
      mockBackend({ project_list: () => projects });
      render(<CreatePlaneDialog initial={null} failure={null} />);
      await userEvent.click(screen.getByRole("tab", { name: "From projects" }));
      const id = screen.getByRole("textbox", { name: /^Id/ });
      await userEvent.type(id, "mine");
      await userEvent.type(screen.getByRole("textbox", { name: "Branch" }), "feat/x");
      expect(id).toHaveValue("mine");
    });

    it("only offers Skip fetch with New or Existing", async () => {
      mockBackend({ project_list: () => projects });
      render(<CreatePlaneDialog initial={null} failure={null} />);
      await userEvent.click(screen.getByRole("tab", { name: "From projects" }));
      await userEvent.click(screen.getByText("Advanced"));
      expect(screen.getByRole("checkbox", { name: "Skip fetch" })).toBeDisabled();
      await userEvent.click(screen.getByRole("radio", { name: "Existing" }));
      expect(screen.getByRole("checkbox", { name: "Skip fetch" })).toBeEnabled();
    });

    it("requires a branch and a member", async () => {
      const backend = mockBackend({ project_list: () => projects });
      render(<CreatePlaneDialog initial={null} failure={null} />);
      await userEvent.click(screen.getByRole("tab", { name: "From projects" }));
      await userEvent.click(screen.getByRole("button", { name: "Create plane" }));
      expect(screen.getByText("Enter a branch")).toBeInTheDocument();
      expect(screen.getByText("Pick at least one project or path")).toBeInTheDocument();
      expect(backend.called("plane_create")).toHaveLength(0);
    });

    it("reopens with the entries and bp's explanation", async () => {
      mockBackend({ project_list: () => projects });
      const { args } = createInfo("feat-x");
      render(
        <CreatePlaneDialog
          initial={{ ...args, members: [{ spec: "@sos/api", branch: "dev" }] }}
          failure={{
            envelope: {
              error: "lock_timeout",
              code: 5,
              message: "another bp holds the lock",
              problems: [{ subject: "feat-x", message: "locked by pid 42" }],
              remedy: "Wait for it to finish.",
            },
            log: null,
            retryable: true,
          }}
        />,
      );
      expect(screen.getByRole("tab", { name: "From projects" })).toHaveAttribute(
        "aria-selected",
        "true",
      );
      expect(screen.getByRole("textbox", { name: "Branch" })).toHaveValue("feat/x");
      expect(screen.getByRole("textbox", { name: /^Id/ })).toHaveValue("feat-x");
      expect(screen.getByRole("textbox", { name: "Branch for @sos/api" })).toHaveValue("dev");
      expect(screen.getByText("another bp holds the lock")).toBeInTheDocument();
      expect(screen.getByText("locked by pid 42")).toBeInTheDocument();
      expect(screen.getByText("Wait for it to finish.")).toBeInTheDocument();
      expect(screen.getByRole("button", { name: "Retry" })).toBeInTheDocument();
      await waitFor(() => expect(screen.getByRole("checkbox", { name: "sos/api" })).toBeChecked());
    });
  });
});
