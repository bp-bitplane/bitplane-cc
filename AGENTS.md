# bitplane-cc (bitplane Command Center)

Desktop app for working with bitplane planes: a sidebar of planes and tabbed terminal sessions rooted in each plane.

## Agent skills

### Issue tracker

GitHub issues across the `bp-bitplane` repos, collected on org Project #1; cross-cutting work goes in `bitplane-protocol`. See `docs/agents/issue-tracker.md`.

### Domain docs

Single-context: `GLOSSARY.md` and `docs/adr/` at the repo root. See `docs/agents/domain.md`.

## The bundled daemon

cc ships `bitplane-sessiond` as a Tauri sidecar (`bundle.externalBin`). `scripts/build-sidecar.sh` (`pnpm sidecar`) builds it into `src-tauri/binaries/` at the `rev` that `src-tauri/Cargo.toml` pins for the `bitplane-sessiond` dependency, so cc's client and its daemon always match. `dev`, `build`, `lint`, `test` and `dmg` run it first, because Tauri's build script fails without the binary.

To build against sibling checkouts instead, create the gitignored `.cargo/config.toml` at the repo root:

```toml
[patch."https://github.com/bp-bitplane/bitplane-sessiond"]
bitplane-sessiond = { path = "../bitplane-sessiond/crates/bitplane-sessiond" }

[patch."https://github.com/bp-bitplane/bitplane-protocol"]
bitplane-protocol = { path = "../bitplane-protocol/crates/bitplane-protocol" }
```

`build-sidecar.sh` then builds the daemon from `../bitplane-sessiond`. The patch rewrites `Cargo.lock`; never commit that version.

## Building

The daemon links libghostty-vt, and so does cc through the `bitplane-sessiond` lib. Building it needs Zig 0.15.2 on `PATH` and sessiond's build environment (the vendored ghostty source and, on macOS, the SDK shim; see sessiond's `docs/build.md`). Run cargo through `scripts/build-env.sh` (`scripts/build-env.sh cargo test --manifest-path src-tauri/Cargo.toml`); the pnpm scripts already do. It uses `../bitplane-sessiond` when the patch above exists, else a checkout of the pinned rev in `src-tauri/target/sessiond-src`.
