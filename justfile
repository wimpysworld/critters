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
    @cargo build --release --manifest-path server/Cargo.toml

# Check what tailor would change and measure
measure:
    @tailor baste
    @tailor measure
