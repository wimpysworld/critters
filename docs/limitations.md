# Limitations

## Zed API limits shape the MVP

Critters is intentionally built around the parts of Zed's extension model that are clearly available today.

That means:

- diagnostics are the primary UI surface
- hover is the secondary UI surface
- rich inline markers are out of scope for now

## No wildcard language attachment

Zed does not offer a verified wildcard entry for attaching an extension language server to every language.

Critters therefore ships with a broad but explicit language list in `extension.toml`.

If Zed gains a first-class wildcard matcher later, Critters should switch to it.

## No built-in managed binary downloads

Critters intentionally does not auto-download and execute the latest GitHub release binary.
That fallback was removed because it could not pin or verify release artifacts before execution.

Use a local `binary.path` override or put `critters-lsp` on your `PATH`.

## No code actions yet

Critters reports suspicious characters and explains them.

It does not yet offer removals, replacements, or quick fixes.
