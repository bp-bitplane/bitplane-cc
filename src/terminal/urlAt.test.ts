import { urlAt } from "./urlAt";

const line = (text: string) => [{ text, wrapsToNext: false }];

describe("urlAt", () => {
  it("finds the URL under the clicked offset", () => {
    const text = "see https://x.dev/a for details";
    expect(urlAt(line(text), 0, text.indexOf("x.dev"))).toBe("https://x.dev/a");
  });

  it("finds nothing when the click misses every URL", () => {
    const text = "see https://x.dev/a for details";
    expect(urlAt(line(text), 0, text.indexOf("details"))).toBeUndefined();
    expect(urlAt(line(text), 0, 0)).toBeUndefined();
  });

  it("ignores schemes other than http and https", () => {
    for (const text of ["file:///etc/passwd", "javascript:alert(1)", "ftp://x.dev/a"]) {
      expect(urlAt(line(text), 0, 3)).toBeUndefined();
    }
  });

  it("drops trailing sentence punctuation", () => {
    for (const end of [".", ",", ";", ":", "!", "?", "...", "?!"]) {
      const text = `go to https://x.dev/a${end} now`;
      expect(urlAt(line(text), 0, 8)).toBe("https://x.dev/a");
    }
  });

  it("keeps punctuation inside the URL", () => {
    const text = "https://x.dev/a.html?q=1;b=2,c";
    expect(urlAt(line(text), 0, 3)).toBe(text);
  });

  it("drops unbalanced closing brackets and quotes", () => {
    const cases: [string, string][] = [
      ["(see https://x.dev/a)", "https://x.dev/a"],
      ["(see https://x.dev/a).", "https://x.dev/a"],
      ["[link](https://x.dev/a)", "https://x.dev/a"],
      ["{https://x.dev/a}", "https://x.dev/a"],
      ["<https://x.dev/a>", "https://x.dev/a"],
      ["{https://x.dev/a}}", "https://x.dev/a"],
      ['"https://x.dev/a"', "https://x.dev/a"],
      ["'https://x.dev/a',", "https://x.dev/a"],
    ];
    for (const [text, url] of cases) {
      expect(urlAt(line(text), 0, text.indexOf("x.dev"))).toBe(url);
    }
  });

  it("keeps balanced brackets in the URL", () => {
    const text = "see https://en.wikipedia.org/wiki/Foo_(bar).";
    expect(urlAt(line(text), 0, 10)).toBe("https://en.wikipedia.org/wiki/Foo_(bar)");
    const nested = "(https://en.wikipedia.org/wiki/Foo_(bar))";
    expect(urlAt(line(nested), 0, 10)).toBe("https://en.wikipedia.org/wiki/Foo_(bar)");
  });

  it("joins soft-wrapped rows so a long URL opens in full", () => {
    const rows = [
      { text: "$ echo hi", wrapsToNext: false },
      { text: "open https://x.dev/", wrapsToNext: true },
      { text: "a/very/long/", wrapsToNext: true },
      { text: "path done", wrapsToNext: false },
      { text: "https://y.dev", wrapsToNext: false },
    ];
    const url = "https://x.dev/a/very/long/path";
    expect(urlAt(rows, 1, 8)).toBe(url);
    expect(urlAt(rows, 2, 0)).toBe(url);
    expect(urlAt(rows, 3, 2)).toBe(url);
    expect(urlAt(rows, 3, 6)).toBeUndefined();
    expect(urlAt(rows, 4, 2)).toBe("https://y.dev");
    expect(urlAt(rows, 0, 2)).toBeUndefined();
  });

  it("does not join rows that end with a hard newline", () => {
    const rows = [
      { text: "https://x.dev/", wrapsToNext: false },
      { text: "more", wrapsToNext: false },
    ];
    expect(urlAt(rows, 0, 3)).toBe("https://x.dev/");
    expect(urlAt(rows, 1, 1)).toBeUndefined();
  });

  it("finds nothing for a bare scheme", () => {
    expect(urlAt(line("https://."), 0, 2)).toBeUndefined();
    expect(urlAt(line("http://a"), 0, 2)).toBe("http://a");
  });

  it("does not match a scheme glued to preceding text", () => {
    const text = "xhttps://x.dev/a";
    expect(urlAt(line(text), 0, 5)).toBeUndefined();
  });
});
