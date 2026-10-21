---
title: Markdown renderer choice
ticket: https://github.com/bp-bitplane/bitplane-cc/issues/38
date: 2026-10-21
status: answered
recommendation: react-markdown 10.1.0 + remark-gfm 4.0.1, fenced code highlighted with @pierre/diffs' shared Shiki highlighter (theme pierre-dark); raw HTML left escaped for now, rehype-raw + rehype-sanitize if HTML must render
related:
  - docs/adr/0009-pierre-diffs-for-file-tabs.md (on the development branch)
  - src/file-tab/DiffView.tsx, src/file-tab/FileTabView.tsx (on the development branch)
---

# Markdown renderer choice

## Question

Which Markdown renderer should the File tab's Markdown preview use? Compared on bundle size, GFM support, safety in the Tauri webview, code-block highlighting consistent with `@pierre/diffs`, theming, and maintenance. Frontend only; relative links/images are out of scope.

Grounding: the File tab lives on `development` (not yet on `main`). `FileTabView.tsx` loads plain files with `commands.planeReadFile(planeId, path)` and renders them with `DiffView kind="file"`, which wraps `@pierre/diffs`' `<File>` (pinned at `1.5.2`, lazy-loaded, `themeType: "dark"`). ADR 0009 makes `DiffView.tsx` the only module that imports `@pierre/diffs`. `pierreTheme.module.css` maps `theme.css` tokens onto `--diffs-*` properties because Pierre renders inside a shadow DOM.

## Recommendation

Use **`react-markdown@10.1.0` + `remark-gfm@4.0.1`**. Render fenced code blocks through a `components.code`/`pre` override that highlights with **`@pierre/diffs`' own `getSharedHighlighter()`**, the same Shiki instance, JS regex engine and `pierre-dark` theme that `<File>`/`<FileDiff>` already use. That gives identical token colours with no second copy of Shiki.

Why this one:

- **Safe by default without a sanitizer.** It renders to React elements, never `dangerouslySetInnerHTML`. Raw HTML is escaped (or dropped with `skipHtml`), and `javascript:` URLs are emptied by `defaultUrlTransform` (verified locally, see [Safety](#safety)). This matters here: the app has `csp: null` and every app command is open to the webview, including `pty_write`. An XSS in a previewed README would amount to shell access.
- **Full GFM** via `remark-gfm`: tables, task lists, strikethrough, autolink literals and footnotes, all verified locally.
- **React-native.** A per-element `components` map makes styling with CSS modules and theme tokens trivial, and lets the code-block override be a normal React component with async highlighting.
- **Upgrade path for HTML.** If READMEs with `<details>`, `<p align=center><img>` etc. need to render, add `rehype-raw@7.0.0` + `rehype-sanitize@6.0.0`. It's the same pipeline with a GitHub-like allowlist schema.
- Cost: about **48 kB min+gzip** (react-markdown + remark-gfm, measured). The view is lazy-loaded like `DiffView`, so startup doesn't pay it.

### Integration shape

Packages (exact pins, matching the repo's style):

```jsonc
"react-markdown": "10.1.0",
"remark-gfm": "4.0.1"
// only if raw HTML must render:
"rehype-raw": "7.0.0",
"rehype-sanitize": "6.0.0"
```

The highlighting helper lives next to `DiffView.tsx` so `@pierre/diffs` stays behind the wrapper (ADR 0009 would need a one-line amendment: "`DiffView.tsx` and `highlight.ts`"):

```ts
// src/file-tab/highlight.ts — the second (and last) module touching @pierre/diffs
import { getSharedHighlighter, type ThemedToken } from "@pierre/diffs";

const THEME = "pierre-dark"; // what <File themeType="dark"> resolves to (DEFAULT_THEMES.dark)

/** Shiki tokens for a fenced block, or null for an unknown/empty language. */
export async function highlight(code: string, lang: string): Promise<ThemedToken[][] | null> {
  if (!lang) return null;
  try {
    const h = await getSharedHighlighter({ themes: [THEME], langs: [lang] });
    return h.codeToTokens(code, { lang, theme: THEME }).tokens;
  } catch {
    return null; // pierre's resolveLanguage throws for languages Shiki doesn't bundle
  }
}
```

```tsx
// src/file-tab/MarkdownView.tsx — lazy-loaded like DiffView
import Markdown from "react-markdown";
import remarkGfm from "remark-gfm";

export default function MarkdownView({ source }: { source: string }) {
  return (
    <div className={styles.markdown}>
      <Markdown remarkPlugins={[remarkGfm]} components={{ pre: Pre, code: InlineCode }}>
        {source}
      </Markdown>
    </div>
  );
}
// Pre: read the child <code>'s `language-xxx` class and text, render plain text first,
// then useEffect → highlight() → <span style={{ color }}> per token (cancel on unmount/change).
```

`FileTabView` picks `MarkdownView` instead of `DiffView kind="file"` for `.md`/`.markdown` when the preview toggle is on, using the same `planeReadFile` contents. No IPC change.

## Comparison table

Sizes are min+gzip, measured with esbuild 0.28 (`--bundle --minify`, React external) + `gzip -9` on 2026-10-21; Bundlephobia figures are given where its API answered.

| | react-markdown + remark-gfm | marked (+ DOMPurify) | markdown-it | micromark + gfm ext | markdown-to-jsx |
|---|---|---|---|---|---|
| Version (released) | 10.1.0 (2025-03-07) + 4.0.1 (2025-02-10) | 18.1.1 (2026-10-12) + 3.4.17 (2026-10-12) | 15.0.3 (2026-10-16) | 4.0.3 (2026-09-26) + 3.0.0 | 9.10.3 (2026-09-15) |
| Size, measured | 47.8 kB (36.8 kB without gfm; 49.7 kB + sanitize; 102 kB + raw + sanitize) | 13.8 kB (25.7 kB with DOMPurify) | 40.9 kB | 21.4 kB | 28.3 kB |
| Size, Bundlephobia | 34.1 + 9.8 kB | 13.8 kB (+ 11.5 kB) | 40.8 kB | 15.2 + 7.1 kB | 28.2 kB |
| Output | React elements | HTML string | HTML string | HTML string | React elements |
| Tables / strikethrough | yes (remark-gfm) | yes (`gfm: true` default) | yes (default preset) | yes | yes |
| Task lists | yes | yes | plugin needed | yes | yes |
| Autolink literals | yes | yes | `linkify: true` (default off) | yes | yes |
| Raw HTML default | escaped as text (`skipHtml` drops it) | **passed through, unsanitized** | dropped/escaped (`html: false`) | escaped (`allowDangerousHtml: false`) | **parsed into JSX**, built-in attribute/URL filtering |
| Needs sanitizer? | no; `rehype-sanitize` only with `rehype-raw` | **yes, DOMPurify on output** | no while `html: false` | no while defaults | no per docs, but it renders HTML it filters itself |
| Code-block hook | `components.code` / `pre` (React) | `renderer.code` → string | `highlight` option → string | none (HTML out) | `overrides.code` / `renderRule` (React) |
| Weekly downloads | 22.7 M / 27.7 M | 51.2 M | 16.6 M | 38.6 M | 2.7 M |
| GitHub stars, last push | 15.9 k, 2026-10-03 | 37.3 k, 2026-10-21 | 22.0 k, 2026-10-16 | 2.2 k, 2026-09-26 | 2.4 k, 2026-09-15 |

## Details per candidate

### react-markdown + remark-gfm (recommended)

- Pipeline: markdown → mdast → hast → React via `hast-util-to-jsx-runtime`. It is "safe by default (no `dangerouslySetInnerHTML` or XSS attacks)". [react-markdown README](https://github.com/remarkjs/react-markdown#readme)
- HTML is escaped, or ignored with `skipHtml`. Rendering it needs `rehype-raw` "(±60kb minzipped)", which measured at about 52 kB here. [react-markdown, Appendix A](https://github.com/remarkjs/react-markdown#appendix-a-html-in-markdown)
- Security: "Overwriting `urlTransform` to something insecure will open you up to XSS… To make sure the content is completely safe, even after what plugins do, use `rehype-sanitize`." [react-markdown, Security](https://github.com/remarkjs/react-markdown#security)
- `MarkdownHooks` supports async rehype plugins via `useEffect`, so `@shikijs/rehype` would also work. It is not needed, because a per-block component keeps the document visible while grammars load. [react-markdown, MarkdownHooks](https://github.com/remarkjs/react-markdown#markdownhooks)
- remark-gfm adds "autolink literals, footnotes, strikethrough, tables, and tasklists". [remark-gfm README](https://github.com/remarkjs/remark-gfm#what-is-this)
- Maintenance: react-markdown 10.1.0 is from 2025-03, and the repo was last pushed 2026-10-03. remark-gfm 4.0.1 is from 2025-02. Both are slow-moving but stable, unified-collective packages with high download counts. ([npm](https://www.npmjs.com/package/react-markdown), [npm](https://www.npmjs.com/package/remark-gfm), `gh api repos/remarkjs/react-markdown`)

### marked

- Smallest (13.8 kB), GFM on by default ("If true, use approved GitHub Flavored Markdown (GFM) specification"). [marked options](https://marked.js.org/using_advanced)
- "Marked does not sanitize the output HTML. Please use a sanitize library, like DOMPurify". The `sanitize` option was removed in v8.0.0. [marked README](https://github.com/markedjs/marked#readme), [marked options](https://marked.js.org/using_advanced)
- Output is an HTML string, so it has to go through `dangerouslySetInnerHTML`. That makes DOMPurify (+11.5–12 kB) mandatory and makes theming and highlighting string-based. Very active (released 2026-10-12).

### markdown-it

- "Safe by default". The default preset has `html: false` and `linkify: false`, with tables and strikethrough on. Task lists need a plugin. [markdown-it README](https://github.com/markdown-it/markdown-it), preset in `dist/markdown-it.mjs`
- The largest of the string renderers (40.9 kB, which includes linkify-it, entities and punycode). HTML string output, so it has the same React/innerHTML drawbacks as marked. Active (15.0.3 on 2026-10-16).

### micromark (+ micromark-extension-gfm)

- The low-level CommonMark tokenizer under remark. "safe (by default)"; `allowDangerousHtml` and `allowDangerousProtocol` both default to `false`. [micromark README](https://github.com/micromark/micromark#readme)
- GFM extension: "autolink literals, footnotes, strikethrough, tables, tagfilter, and tasklists". [micromark-extension-gfm](https://github.com/micromark/micromark-extension-gfm#readme)
- HTML-string output with no AST hooks for React components. remark-gfm's own README points to micromark for "*just* turn markdown into HTML". That's a poor fit for React-rendered, themed, async-highlighted blocks.

### markdown-to-jsx

- React output, compact (28.3 kB), GFM tables, task lists, strikethrough and autolinks. [markdown-to-jsx README](https://github.com/quantizor/markdown-to-jsx)
- **Parses raw HTML into JSX by default** (`disableParsingRawHTML` defaults to `false`). It filters `on*` attributes, `javascript:`/`vbscript:`/non-image `data:` URLs and `srcdoc`, and tag-filters `script`/`iframe`. It warns that `data:image/svg+xml` stays allowed. [markdown-to-jsx README, security](https://github.com/quantizor/markdown-to-jsx)
- Locally, `<img src=x onerror=…>` became `<img src="x">` and `<script>` was escaped. It works, but the safety rests on a single-maintainer custom filter rather than on not rendering HTML at all. Smaller ecosystem (2.7 M/week). A good runner-up if bundle size ever matters more than the sanitizer story.

## Highlighting with @pierre/diffs

- `@pierre/diffs` is "built on Shiki". [@pierre/diffs README](https://github.com/pierrecomputer/pierre). Version 1.5.2 depends on `shiki: ^3.0.0 || ^4.0.0`, and the lockfile on `development` resolves `shiki@4.5.0` (`npm view @pierre/diffs@1.5.2 dependencies`, `pnpm-lock.yaml`).
- It exports its module-level singleton: `getSharedHighlighter({ themes, langs, preferredHighlighter = "shiki-js" })`. This lazily creates one Shiki highlighter with `createJavaScriptRegexEngine()` (no WASM), loads missing languages and themes on demand, and returns a Shiki `HighlighterGeneric`. The `<File>`/`<FileDiff>` renderers use the same instance (`dist/highlighter/shared_highlighter.js`, `dist/renderers/FileRenderer.js` in the 1.5.2 tarball).
- The default themes are `{ dark: "pierre-dark", light: "pierre-light" }` (`DEFAULT_THEMES`, `dist/constants.js`). They are registered from `@pierre/theming` when the highlighter module loads. The app sets `themeType: "dark"`, so file views use `pierre-dark`, and fenced blocks should too.
- Once languages are loaded, the instance's `codeToTokens` and `codeToHast` are synchronous (`@shikijs/types@4.5.0`: `codeToTokens: (code, options) => TokensResult`). Shiki: "use `highlighter.codeToHtml` synchronously with the loaded themes and languages", and "codeToTokens … get the intermediate data structure, and render them by yourself". [Shiki install guide](https://shiki.style/guide/install)
- Pierre's `resolveLanguage` throws for a language not in Shiki's `bundledLanguages` (or registered custom ones). The helper must catch that and fall back to plain text (`dist/highlighter/languages/resolveLanguage.js`).
- Alternative: `@shikijs/rehype/core`'s `rehypeShikiFromHighlighter` accepts an existing highlighter. [Shiki rehype](https://shiki.style/packages/rehype). It would need `MarkdownHooks`, which blocks the whole document on grammar loading, plus a direct `@shikijs/rehype` dependency. The per-block `codeToTokens` component avoids both.
- Rejected: rendering each fence with Pierre's `<File>` gives one shadow DOM, header and gutter per block. That's too heavy and looks like a file view rather than prose.
- Importing `shiki` directly is also rejected: it isn't a direct dependency (pnpm strict), and it would risk a second Shiki instance and version drift from Pierre's.

Styling: the Markdown body is ordinary light DOM, so a `MarkdownView.module.css` uses `theme.css` tokens directly (`--text`, `--dim`, `--line`, `--panel`, `--accent`, `--mono`, `--sans`). Tables, task-list checkboxes (`.contains-task-list`, `.task-list-item`, emitted by remark-gfm) and `pre` backgrounds (`--panel`/`--raised`) are styled there. Token colours come inline from `pierre-dark`, and the `pre` background is ours, matching `--diffs-dark-bg: var(--bg)` in `pierreTheme.module.css`.

## Safety

- The webview has **no CSP**: `"security": { "csp": null }` in `src-tauri/tauri.conf.json`. Tauri: "The CSP protection is only enabled if set on the Tauri configuration file". [Tauri CSP](https://v2.tauri.app/security/csp/)
- App commands are callable from any script in the window: "By default, all commands that you registered in your app" are allowed for all windows. [Tauri capabilities](https://v2.tauri.app/security/capabilities/). The app uses a plain `tauri_build::build()` with no `AppManifest`, and exposes `pty_write`, `plane_create`, `plane_read_file`, `plane_open_file` and more (`src-tauri/src/ipc.rs`, `src/ipc/bindings.ts` on `development`). **Script injection from a previewed file would give the attacker the user's terminal.** Markdown in agent-made worktrees is untrusted input.
- Therefore: never use `dangerouslySetInnerHTML` for Markdown. That rules out marked, markdown-it and micromark, or forces DOMPurify into the critical path. Keep react-markdown's default `urlTransform`, and only add `rehype-raw` together with `rehype-sanitize` (sanitize last).
- Verified locally (react-dom/server, 2026-10-21) with react-markdown + remark-gfm:
  - `<img onerror>` and `<script>` came out as escaped text, and `[x](javascript:alert(3))` came out as `<a href="">`.
  - With `rehype-raw` + `rehype-sanitize`, `<details>`, `<p align="center"><img width>` rendered. `onerror`, `<script>` and `<iframe>` were removed, and `href="javascript:…"` was dropped.
- Links: clicks on `<a href>` would navigate the webview. The prototype should intercept them in a `components.a` override (open externally or ignore). Relative links are out of scope for this ticket.

## Sources

- npm registry: `npm view <pkg> version time --json` for react-markdown, remark-gfm, rehype-sanitize, rehype-raw, marked, markdown-it, micromark, micromark-extension-gfm, markdown-to-jsx, dompurify, shiki, @pierre/diffs (run 2026-10-21)
- npm downloads API: https://api.npmjs.org/downloads/point/last-week/{pkg}
- Bundlephobia API: https://bundlephobia.com/api/size?package={pkg}@{version}
- Local esbuild measurements (esbuild 0.28.2, React externalized, `gzip -9`)
- `gh api repos/{owner}/{repo}` for remarkjs/react-markdown, remarkjs/remark-gfm, rehypejs/rehype-sanitize, markedjs/marked, markdown-it/markdown-it, micromark/micromark, quantizor/markdown-to-jsx, shikijs/shiki, pierrecomputer/pierre
- https://github.com/remarkjs/react-markdown#readme
- https://github.com/remarkjs/remark-gfm#readme
- https://github.com/rehypejs/rehype-sanitize
- https://github.com/rehypejs/rehype-raw
- https://marked.js.org/using_advanced and https://github.com/markedjs/marked#readme
- https://github.com/markdown-it/markdown-it
- https://github.com/micromark/micromark#readme, https://github.com/micromark/micromark-extension-gfm#readme
- https://github.com/quantizor/markdown-to-jsx
- https://github.com/pierrecomputer/pierre (@pierre/diffs 1.5.2 tarball, `npm pack @pierre/diffs@1.5.2`: `dist/highlighter/shared_highlighter.js`, `dist/constants.js`, `dist/highlighter/languages/resolveLanguage.js`, `dist/types.d.ts`)
- https://shiki.style/guide/install, https://shiki.style/packages/rehype, `@shikijs/types@4.5.0` `dist/index.d.mts`
- https://v2.tauri.app/security/csp/, https://v2.tauri.app/security/capabilities/
- Repo (`origin/development`): `package.json`, `pnpm-lock.yaml`, `src/file-tab/DiffView.tsx`, `src/file-tab/FileTabView.tsx`, `src/ui/pierreTheme.module.css`, `src/theme.css`, `src/ipc/bindings.ts`, `src-tauri/src/ipc.rs`, `src-tauri/tauri.conf.json`, `src-tauri/capabilities/default.json`, `docs/adr/0009-pierre-diffs-for-file-tabs.md`
