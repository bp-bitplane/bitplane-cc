import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { fileTabsOf } from "../store/selectors";
import { useStore } from "../store/store";
import { flush, mockBackend, plane, planesModel, seed } from "../test/fixtures";
import MarkdownView from "./MarkdownView";

vi.mock("./DiffView", () => ({ highlightCode: async () => null }));

const DOC = [
  "# Guide",
  "",
  "See [the readme](../README.md), [my notes](my%20notes.md#top), [api](api/),",
  "[up](../../x.md) and [the site](https://example.com).",
  "",
  "Jump to [installation](#installation) or a note[^1].",
  "",
  "## Installation",
  "",
  "![diagram](./img/flow.png) ![](/assets/x.svg) ![badge](https://img.example/b.svg)",
  "",
  "[^1]: A footnote.",
  "",
].join("\n");

describe("MarkdownView", () => {
  let scrolled: Element[];

  beforeEach(() => {
    seed({ planes: planesModel([plane("a")]) });
    scrolled = [];
    Element.prototype.scrollIntoView = vi.fn(function (this: Element) {
      scrolled.push(this);
    });
  });

  const renderDoc = () => render(<MarkdownView planeId="a" path="docs/guide.md" source={DOC} />);
  const openPaths = () => fileTabsOf(useStore.getState(), "a").map((tab) => tab.path);

  it("opens a relative link as a File tab, resolved against the file's folder", async () => {
    const backend = mockBackend({ plane_list_dir: () => [] });
    renderDoc();
    expect(fireEvent.click(screen.getByRole("link", { name: "the readme" }))).toBe(false);
    await waitFor(() => expect(openPaths()).toEqual(["README.md"]));
    expect(backend.called("plane_list_dir")[0]?.args).toEqual({ planeId: "a", path: "" });
    expect(useStore.getState().activeTab.a).toBe("file:README.md");
  });

  it("percent-decodes a link and drops its fragment", async () => {
    mockBackend({ plane_list_dir: () => [] });
    renderDoc();
    fireEvent.click(screen.getByRole("link", { name: "my notes" }));
    await waitFor(() => expect(openPaths()).toEqual(["docs/my notes.md"]));
  });

  it("opens a link to a missing file, for its error state", async () => {
    mockBackend({
      plane_list_dir: () => {
        throw "no such folder";
      },
    });
    renderDoc();
    fireEvent.click(screen.getByRole("link", { name: "the readme" }));
    await waitFor(() => expect(openPaths()).toEqual(["README.md"]));
  });

  it("does nothing for a folder link, by trailing / or by listing", async () => {
    mockBackend({
      plane_list_dir: () => [{ name: "README.md", kind: "directory", ignored: false }],
    });
    renderDoc();
    fireEvent.click(screen.getByRole("link", { name: "api" }));
    fireEvent.click(screen.getByRole("link", { name: "the readme" }));
    await flush();
    expect(openPaths()).toEqual([]);
  });

  it("does nothing for a link above the plane root", async () => {
    const backend = mockBackend();
    renderDoc();
    expect(fireEvent.click(screen.getByRole("link", { name: "up" }))).toBe(false);
    await flush();
    expect(backend.calls).toEqual([]);
    expect(openPaths()).toEqual([]);
  });

  it("shows an external link's URL and doesn't follow it", async () => {
    const backend = mockBackend();
    renderDoc();
    const link = screen.getByRole("link", { name: "the site" });
    expect(link).toHaveAttribute("title", "https://example.com");
    expect(fireEvent.click(link)).toBe(false);
    await flush();
    expect(backend.calls).toEqual([]);
    expect(openPaths()).toEqual([]);
  });

  it("gives headings GitHub-style ids and scrolls to an anchor", () => {
    renderDoc();
    const heading = screen.getByRole("heading", { name: "Installation" });
    expect(heading).toHaveAttribute("id", "installation");
    expect(fireEvent.click(screen.getByRole("link", { name: "installation" }))).toBe(false);
    expect(scrolled).toEqual([heading]);
    expect(Element.prototype.scrollIntoView).toHaveBeenCalledWith({
      behavior: "smooth",
      block: "start",
    });
  });

  it("scrolls between a footnote and its back-reference", () => {
    const { container } = renderDoc();
    const ref = container.querySelector<HTMLElement>("a[data-footnote-ref]");
    const backref = container.querySelector<HTMLElement>("a[data-footnote-backref]");
    if (!ref || !backref) throw new Error("no footnote");
    fireEvent.click(ref);
    fireEvent.click(backref);
    expect(scrolled.map((element) => element.id)).toEqual(["user-content-fn-1", ref.id]);
  });

  it("tries the user-content- prefix and ignores a missing anchor", () => {
    render(
      <MarkdownView
        planeId="a"
        path="x.md"
        source={"[a](#fn-1) [b](#nowhere) x[^1]\n\n[^1]: Note."}
      />,
    );
    fireEvent.click(screen.getByRole("link", { name: "a" }));
    fireEvent.click(screen.getByRole("link", { name: "b" }));
    expect(scrolled.map((element) => element.id)).toEqual(["user-content-fn-1"]);
  });

  it("shows a local image as a chip with its alt text and path, and loads a remote one", () => {
    const { container } = renderDoc();
    const images = container.querySelectorAll("img");
    expect(images).toHaveLength(1);
    expect(images[0]).toHaveAttribute("src", "https://img.example/b.svg");
    const diagram = screen.getByRole("img", { name: "diagram" });
    expect(diagram.tagName).toBe("SPAN");
    expect(diagram).toHaveTextContent("diagram./img/flow.png");
    expect(screen.getByRole("img", { name: "/assets/x.svg" })).toHaveTextContent("/assets/x.svg");
  });
});
