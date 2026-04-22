# Configuration

Critters reads Zed settings from `lsp.critters-lsp.settings`.

## Top-level keys

### `max_diagnostics_per_document`

Caps the number of published diagnostics for a single document.

Default:

```json
500
```

### `rules`

Adds or overrides tracked characters.

Keys are uppercase or lowercase hexadecimal code points, or inclusive hexadecimal ranges.

Examples:

- `"00A0"`
- `"200B"`
- `"202A-202E"`
- `"0080-00FF"`

Rule shape:

```json
{
  "description": "NO-BREAK SPACE",
  "severity": "warning",
  "class": "spacing",
  "zero_width": false
}
```

Field meanings:

- `description` - display label used in diagnostics and hover
- `severity` - one of `none`, `info`, `warning`, `error`
- `class` - freeform grouping label used in hover output
- `zero_width` - whether the character should be treated as zero-width in descriptions

`severity: "none"` removes a built-in rule or a broader custom rule for that code point.

When custom rules overlap, Critters applies broader ranges first and narrower rules last, so the most specific rule wins.
Inclusive ranges that would expand to more than `4096` code points are rejected.

### `language_overrides`

Overrides rules for a specific LSP `languageId`.

Example:

```json
{
  "language_overrides": {
    "markdown": {
      "rules": {
        "00A0": {
          "severity": "none"
        }
      }
    },
    "rust": {
      "rules": {
        "2013": {
          "severity": "error",
          "description": "EN DASH IN SOURCE"
        }
      }
    }
  }
}
```

## Nested compatibility form

Critters also accepts a nested form if your client sends the server settings wrapped under `critters-lsp`.

```json
{
  "critters-lsp": {
    "rules": {
      "200B": {
        "severity": "error"
      }
    }
  }
}
```

## Suggested Zed snippet

```json
{
  "lsp": {
    "critters-lsp": {
      "settings": {
        "rules": {
          "0080-00FF": {
            "description": "LATIN-1 SUPPLEMENT",
            "severity": "error",
            "class": "latin-1"
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
