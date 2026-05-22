# crates/turbo-bible-tui/assets/

Compressed translation files (`*.db.zst`) the TUI embeds via
`include_bytes!` and decompresses into
`$XDG_DATA_HOME/turbo-bible/translations/` on first launch.

The `.db.zst` files themselves are **gitignored**; they are
regenerable artifacts of the data pipeline. Populate this directory
from a fresh build:

```sh
just bundle-translations [path/to/scrollmapper/checkout]
```

That recipe:

1. Runs `turbo-bible-data build` against the scrollmapper checkout +
   `data/manifest_source.toml`.
2. Runs `turbo-bible-data compress`.
3. Copies `dist/translations/*.db.zst` into this directory.

If you `cargo build -p turbo-bible` with this directory empty, the
build fails (loudly) at the `include_bytes!` site in
`src/bundled.rs` — that's intentional: a stripped binary would not
have any translations to install.
