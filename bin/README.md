# bin/

`ai-mouse-overlay.exe` is built here by `pnpm run build:overlay`
(`cargo build --release` in `overlay/`, then copied).

The file is git-ignored, but it **must be present in the published npm package** —
end users install the plugin without a Rust toolchain. `prepack` builds it when missing.
