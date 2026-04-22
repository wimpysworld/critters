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

## Managed binaries depend on GitHub releases

The extension can download a managed `critters-lsp` binary from GitHub releases, but that only works after tagged release assets exist.

During early development, use a local `binary.path` override or put `critters-lsp` on your `PATH`.

## No code actions yet

Critters reports suspicious characters and explains them.

It does not yet offer removals, replacements, or quick fixes.
