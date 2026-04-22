# Critters

Critters brings the core safety behaviour of `vscode-gremlins` to Zed.

It scans open files for invisible, misleading, and high-risk Unicode characters, then reports them through Zed diagnostics and hover.

## What v0.1 ships

- suspicious Unicode detection in supported languages
- built-in rules for zero-width characters, bidi controls, non-breaking spaces, soft hyphens, and a conservative typography set
- configurable rules keyed by hexadecimal code point or range
- language-specific overrides keyed by LSP `languageId`
- one diagnostic per contiguous suspicious run
- hover details with code points, classes, and severity
- Zed extension wrapper that can launch a managed `critters-lsp` binary or use a locally installed one

## What it does not promise yet

- arbitrary inline decorations
- gutter markers
- overview ruler styling
- semantic token styling

Those are current parity gaps against `vscode-gremlins`, and they are documented rather than hand-waved.

## Configuration

Add settings under `lsp.critters-lsp.settings` in `~/.config/zed/settings.json`.

```json
{
  "lsp": {
    "critters-lsp": {
      "settings": {
        "max_diagnostics_per_document": 250,
        "rules": {
          "0080-00FF": {
            "description": "LATIN-1 SUPPLEMENT",
            "severity": "error",
            "class": "latin-1",
            "zero_width": false
          }
        },
        "language_overrides": {
          "markdown": {
            "rules": {
              "00A0": {
                "severity": "none"
              }
            }
          }
        }
      }
    }
  }
}
```

## Local development

1. Enter the dev shell.
2. Build the server.
3. Install the repo as a dev extension in Zed.
4. Point Zed at the locally built binary, or put `critters-lsp` on your `PATH`.

```bash
nix develop
cargo build --manifest-path server/Cargo.toml
```

Example local binary override:

```json
{
  "lsp": {
    "critters-lsp": {
      "binary": {
        "path": "/absolute/path/to/critters/server/target/debug/critters-lsp"
      }
    }
  }
}
```

## Managed binaries

The extension looks for `critters-lsp` in this order:

1. `lsp.critters-lsp.binary.path`
2. `critters-lsp` on the worktree `PATH`
3. common local development build paths
4. the latest GitHub release asset from this repository

Managed release assets are expected to be named like:

- `critters-lsp-v0.1.0-x86_64-unknown-linux-gnu.tar.gz`
- `critters-lsp-v0.1.0-aarch64-apple-darwin.tar.gz`
- `critters-lsp-v0.1.0-x86_64-pc-windows-msvc.zip`

## Documentation

- [Configuration](docs/configuration.md)
- [Parity matrix](docs/parity-matrix.md)
- [Limitations](docs/limitations.md)
