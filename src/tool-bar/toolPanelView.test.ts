import { shownPlaneId } from "../store/selectors";
import { useStore } from "../store/store";
import { createInfo, plane, planesModel, seed, session, sessionsById } from "../test/fixtures";
import { toolPanelView } from "./toolPanelView";

const view = () => toolPanelView(useStore.getState());

describe("shownPlaneId", () => {
  it("follows the active tab, then the Sidebar selection", () => {
    seed({
      sessions: sessionsById(session(1, "a")),
      selectedPlaneId: "a",
      activeTab: { a: 1 },
    });
    expect(shownPlaneId(useStore.getState())).toBe("a");
    seed({ selectedPlaneId: "b" });
    expect(shownPlaneId(useStore.getState())).toBe("b");
    seed();
    expect(shownPlaneId(useStore.getState())).toBeNull();
  });
});

describe("toolPanelView", () => {
  it("is empty without a plane", () => {
    seed({ planes: planesModel([plane("a")]) });
    expect(view()).toEqual({ planeId: null, screen: "empty" });
  });

  it("shows tools for a plane or a draft", () => {
    seed({ planes: planesModel([plane("a"), plane("d", "draft")]), selectedPlaneId: "a" });
    expect(view()).toEqual({ planeId: "a", screen: null });
    seed({ planes: planesModel([plane("d", "draft")]), selectedPlaneId: "d" });
    expect(view()).toEqual({ planeId: "d", screen: null });
  });

  it("says a gone plane is gone", () => {
    seed({
      planes: planesModel([plane("old", "gone")]),
      sessions: sessionsById(session(1, "old")),
      selectedPlaneId: "old",
    });
    expect(view()).toEqual({ planeId: "old", screen: "gone" });
    seed({ planes: planesModel([]), selectedPlaneId: "missing" });
    expect(view()).toEqual({ planeId: "missing", screen: "gone" });
  });

  it("waits for the plane list before calling a plane gone", () => {
    seed({ planes: planesModel([], { status: { kind: "loading" } }), selectedPlaneId: "a" });
    expect(view()).toEqual({ planeId: "a", screen: "empty" });
  });

  it("holds tools back while a plane is created", () => {
    seed({ planes: planesModel([plane("new", "creating")]), selectedPlaneId: "new" });
    expect(view()).toEqual({ planeId: "new", screen: "creating" });
    seed({ planes: planesModel([]), creates: { new: createInfo("new") }, selectedPlaneId: "new" });
    expect(view()).toEqual({ planeId: "new", screen: "creating" });
  });
});
