import { render, screen, within } from "@testing-library/react";
import { mockBackend, seed } from "../test/fixtures";
import { Dialogs } from "./Dialogs";

describe("daemon info dialog", () => {
  it("shows the process, its paths, the live status and the last hello reply", () => {
    mockBackend();
    seed({
      daemon: {
        kind: "connected",
        version: "0.1.0",
        instance: "i1",
        restarted: false,
        modules: [],
      },
      dialog: {
        kind: "daemonInfo",
        info: {
          hello: '{ "protocolVersion": 1 }',
          pid: 42,
          executable: "/usr/local/bin/bitplane-sessiond",
          bundled: "/cc/bitplane-sessiond",
          socket: "/run/sessiond.sock",
          log: "/state/sessiond.log",
        },
      },
    });
    render(<Dialogs />);
    const dialog = screen.getByRole("dialog", { name: "Session daemon" });
    expect(within(dialog).getByText("42")).toBeInTheDocument();
    expect(within(dialog).getByText("/usr/local/bin/bitplane-sessiond")).toBeInTheDocument();
    expect(within(dialog).getByText("/run/sessiond.sock")).toBeInTheDocument();
    expect(within(dialog).getByText(/"instance": "i1"/)).toBeInTheDocument();
    expect(within(dialog).getByText('{ "protocolVersion": 1 }')).toBeInTheDocument();
  });

  it("says what it doesn't know before the first hello", () => {
    mockBackend();
    seed({ dialog: { kind: "daemonInfo", info: null } });
    render(<Dialogs />);
    expect(screen.getByText("No hello reply yet.")).toBeInTheDocument();
    expect(screen.getAllByText("unknown")).toHaveLength(5);
  });
});
