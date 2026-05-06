# Parity matrix

| Feature | `vscode-gremlins` | Critters v0.1 | Notes |
| --- | --- | --- | --- |
| Default suspicious-character detection | Yes | Yes | Conservative built-in rules cover the highest-value invisible and misleading characters |
| Custom code-point rules | Yes | Yes | `rules` supports single code points |
| Custom range rules | Yes | Yes | `rules` supports inclusive hexadecimal ranges |
| Severity levels | Yes | Yes | `none`, `info`, `warning`, `error` |
| Language-specific overrides | Yes | Yes | Keyed by LSP `languageId` |
| Problems integration | Optional | Yes | Published through LSP diagnostics |
| Hover details | Yes | Yes | Hover reports code points, classes, and severity |
| One issue per contiguous run | Noisy in some editors | Yes | Runs are grouped before diagnostics are published |
| Code actions / quick fixes | Yes | Yes | Removes invisible controls and replaces safe ASCII equivalents where Critters can do so without guessing |
| Gutter/lightbulb affordance | Yes | Yes | Exposed through LSP diagnostics plus quick fixes, matching Zed's supported Harper-style interaction |
| Arbitrary inline decorations | Yes | No | Current Zed extension APIs do not expose a verified public path for this |
| Zero-width red-bar rendering | Yes | No | Same API gap |
| Gutter icon per affected line | Yes | Partial | Zed shows diagnostics and quick-fix affordances in the gutter, but not custom Critters icons |
| Overview ruler styling | Yes | No | Same API gap |
| Semantic-token styling | No | Not yet | Left for a later experiment |
