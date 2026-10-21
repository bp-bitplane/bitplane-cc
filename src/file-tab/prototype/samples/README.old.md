# bitplane-cc

A desktop workspace for **bitplane**: pick a plane, work in terminal sessions rooted in it.
Built with [Tauri 2](https://v2.tauri.app), React 19 and Vite. See the [glossary](./GLOSSARY.md)
and the [ADRs](docs/adr/) for the vocabulary, or <https://github.com/bp-bitplane/bitplane-cc>.

> **Note**
> This README is the sample file of the Markdown preview prototype (#37). Links don't navigate;
> clicks are logged to the console.

## Getting started

1. Install Node 24 and `pnpm`.
2. Run `pnpm install`.
3. Start the app with `pnpm dev`, or only the frontend with `pnpm vite:dev`.

## Roadmap

- [x] Sidebar with planes and their tabs
- [x] File tabs with diffs (ADR 0009)
- [ ] Markdown preview
- [ ] ~~Built-in editor~~ (out of scope)

## Commands

| Script | What it does | Needs Rust |
| :-- | :-- | :-: |
| `pnpm dev` | Tauri app with hot reload | yes |
| `pnpm vite:dev` | Frontend only, on port 1420 | no |
| `pnpm test` | Vitest, then `cargo test` | yes |

## Examples

Open a File tab from TypeScript:

```ts
import { openFileTab } from "./store/store";

export function openReadme(planeId: string): void {
  // Opens as the plane's preview until pinned.
  openFileTab({ planeId, path: "README.md", mode: "file", pinned: false });
}
```

The backend side, in Rust:

```rust
#[tauri::command]
pub async fn plane_read_file(plane_id: String, path: String) -> Result<FileText, String> {
    let root = plane_root(&plane_id)?;
    read_text(root.join(&path), MAX_BYTES).map_err(|e| e.to_string())
}
```

A shell session:

```bash
bp create --here
for member in $(bp members); do
  git -C "$member" status --short
done
```

Config, with a language Shiki doesn't know (plain text fallback):

```made-up-lang
key = value   # no highlighting expected
```

Raw HTML stays text: <details><summary>click</summary>hidden</details> and <script>alert(1)</script>.

---

Footnotes work too.[^1] Inline `code`, *emphasis*, **strong** and ~~strikethrough~~.

[^1]: Rendered by `react-markdown` + `remark-gfm`.
