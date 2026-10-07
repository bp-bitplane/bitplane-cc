import { useStore } from "../store/store";
import { createInfo, plane, planesModel, seed, session, sessionsById } from "../test/fixtures";
import { mainView } from "./MainArea";

const view = () => {
  const { tabs, screen } = mainView(useStore.getState());
  return { tabs, screen };
};

describe("mainView", () => {
  it("blocks on bp and git problems", () => {
    seed({ planes: planesModel([plane("a")], { status: { kind: "bpNotFound" } }) });
    expect(view()).toEqual({ tabs: null, screen: "status" });
  });

  it("invites creating the first plane", () => {
    seed({ planes: planesModel([]) });
    expect(view()).toEqual({ tabs: null, screen: "noPlanes" });
  });

  it("hints at the sidebar at startup instead of opening anything", () => {
    seed({ planes: planesModel([plane("a")]) });
    expect(view()).toEqual({ tabs: null, screen: "hint" });
  });

  it("shows the terminal when the selected plane has sessions", () => {
    seed({
      planes: planesModel([plane("a")]),
      sessions: sessionsById(session(1, "a")),
      selectedPlaneId: "a",
    });
    expect(view()).toEqual({ tabs: "a", screen: null });
  });

  it("offers a new session once the last one closed", () => {
    seed({ planes: planesModel([plane("a")]), selectedPlaneId: "a" });
    expect(view()).toEqual({ tabs: "a", screen: "noSessions" });
  });

  it("shows progress for a plane being created", () => {
    seed({ planes: planesModel([plane("a", "creating")]), selectedPlaneId: "a" });
    expect(view()).toEqual({ tabs: null, screen: "creating" });
  });

  it("shows progress before the creating row arrives", () => {
    seed({ planes: planesModel([]), creates: { a: createInfo("a") }, selectedPlaneId: "a" });
    expect(view()).toEqual({ tabs: null, screen: "creating" });
  });

  it("says a gone plane without sessions is gone", () => {
    seed({ planes: planesModel([]), selectedPlaneId: "a" });
    expect(view()).toEqual({ tabs: null, screen: "gone" });
  });
});
