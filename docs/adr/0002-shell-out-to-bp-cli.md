# Shell out to the bp CLI instead of linking bitplane-core

The app talks to bitplane only by running `bp … --json` and parsing its output. It does not link `bitplane-core`. That crate is `publish = false` and its internals are unstable, while the CLI's JSON output and exit codes are the contract bitplane actually maintains. The cost is one process per call, plus parsing stderr for errors and progress. Gaps in the CLI are worked around in the app and listed as upstream wishes, rather than closed by reaching into the core.
