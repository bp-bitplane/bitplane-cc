import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useStore } from "../store/store";
import { mockBackend, plane, planesModel, seed, session, sessionsById } from "../test/fixtures";
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
    expect(useStore.getState().dialog).toBeNull();
  });

  it("shows session counts and findings", () => {
    render(<Palette />);
    expect(screen.getByTitle("2 open sessions")).toBeInTheDocument();
    expect(screen.getByRole("img", { name: "findings" })).toBeInTheDocument();
  });

  it("says when nothing matches and closes on Escape", async () => {
    render(<Palette />);
    await userEvent.keyboard("zzz");
    expect(screen.getByText("No plane matches")).toBeInTheDocument();
    await userEvent.keyboard("{Escape}");
    expect(useStore.getState().dialog).toBeNull();
  });
});
