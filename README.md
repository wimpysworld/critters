# Critters

Critters brings the core safety behaviour of `vscode-gremlins` to Zed.

It scans open files for invisible, misleading, and high-risk Unicode characters, then reports them through Zed diagnostics, hover, and quick fixes.

## What v0.1 ships

- suspicious Unicode detection in supported languages
- built-in rules for zero-width characters, bidi controls, non-breaking spaces, soft hyphens, and a conservative typography set
- configurable rules keyed by hexadecimal code point or range
- language-specific overrides keyed by LSP `languageId`
- one diagnostic per contiguous suspicious run
- hover details with code points, classes, and severity
- Zed extension wrapper that launches a configured or locally installed `critters-lsp` binary
- quick fixes that remove invisible controls or replace safe cases such as no-break spaces and curly quotes with ASCII

## What it does not promise yet

- arbitrary inline decorations
- custom gutter icons
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

## Documentation

- [Configuration](docs/configuration.md)
- [Parity matrix](docs/parity-matrix.md)
- [Limitations](docs/limitations.md)
