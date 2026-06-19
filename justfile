# List available recipes
default:
    @just --list

# Alter tailor swatches
alter:
    @tailor alter

# Run linters
lint:
    @actionlint
    @cargo fmt --all --check
    @cargo clippy --workspace --all-targets -- -D warnings

# Run tests
test:
    @cargo test --workspace

# Check the full workspace
check:
    @cargo check --workspace

# Format Rust sources
fmt:
    @cargo fmt --all

# Build the language server release binary
build-server:
    @cargo build --release --package critters-lsp

# Build the Zed extension WebAssembly module
build-zed:
    @rustup target add wasm32-wasip2
    @cargo build --package critters-zed --target wasm32-wasip2

# Show package version from critters-lsp
version:
    @grep '^version = ' crates/critters-lsp/Cargo.toml | head -1 | cut -d'"' -f2

# List recent releases
releases:
    @git tag --sort=-creatordate | head -10

# Show what would be in the next release changelog
changelog:
    #!/usr/bin/env bash
    latest_tag=$(git describe --tags --abbrev=0 2>/dev/null || echo "")
    if [ -n "$latest_tag" ]; then
        echo "Changes since $latest_tag:"
        git log "$latest_tag"..HEAD --pretty=format:"* %s (%h)"
    else
        echo "No previous tags found. All commits:"
        git log --pretty=format:"* %s (%h)"
    fi

# Create a new release tag (VERSION must be x.y.z, without a v prefix)
release VERSION:
    #!/usr/bin/env bash
    set -euo pipefail

    version="{{VERSION}}"

    if ! echo "$version" | grep -qE '^[0-9]+\.[0-9]+\.[0-9]+$'; then
        echo "Error: VERSION must be bare semver, for example 0.1.0"
        exit 1
    fi

    if [ -n "$(git status --porcelain)" ]; then
        echo "Error: working directory has uncommitted changes"
        exit 1
    fi

    if git rev-parse "$version" >/dev/null 2>&1; then
        echo "Error: tag $version already exists locally"
        exit 1
    fi

    if git ls-remote --tags origin "refs/tags/$version" | grep -q "refs/tags/$version"; then
        echo "Error: tag $version already exists on origin"
        exit 1
    fi

    check_toml_version() {
        path="$1"
        actual=$(grep '^version = ' "$path" | head -1 | cut -d'"' -f2)
        if [ "$actual" != "$version" ]; then
            echo "Error: $path version is $actual, expected $version"
            exit 1
        fi
    }

    check_lock_version() {
        package="$1"
        actual=$(awk -v package="$package" '
            $0 == "[[package]]" { in_package = 0 }
            $0 == "name = \"" package "\"" { in_package = 1 }
            in_package && /^version = / { gsub(/"/, "", $3); print $3; exit }
        ' Cargo.lock)
        if [ "$actual" != "$version" ]; then
            echo "Error: Cargo.lock $package version is $actual, expected $version"
            exit 1
        fi
    }

    check_toml_version crates/critters-core/Cargo.toml
    check_toml_version crates/critters-lsp/Cargo.toml
    check_toml_version editors/zed/Cargo.toml

    extension_version=$(grep '^version = ' editors/zed/extension.toml | head -1 | cut -d'"' -f2)
    if [ "$extension_version" != "$version" ]; then
        echo "Error: editors/zed/extension.toml version is $extension_version, expected $version"
        exit 1
    fi

    check_lock_version critters-core
    check_lock_version critters-lsp
    check_lock_version critters-zed

    echo "Creating release $version..."
    git tag -a "$version" -m "Release $version"
    echo "✓ Tag $version created"
    echo ""
    echo "To publish the release:"
    echo "  git push origin $version"
    echo ""
    echo "This will trigger GitHub Actions to:"
    echo "  - run the sentinel-gated Builder workflow"
    echo "  - build critters-lsp for Windows, macOS, and Linux"
    echo "  - create a GitHub Release with downloadable assets"

# Check what tailor would change and measure
measure:
    @tailor baste
    @tailor measure
