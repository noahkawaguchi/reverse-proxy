proxy-port := '3000'
backend-port := '8000'

everything-flags := '--workspace --all-targets'

# Run a simple demo of the proxying behavior (default recipe)
demo:
    #!/usr/bin/env bash
    trap 'kill $(jobs -p)' EXIT

    printf '\nRunning Python backend on port {{ backend-port }}\n\n'
    python3 -m http.server {{ backend-port }} &

    printf 'Running reverse proxy on port {{ proxy-port }}\n\n'
    cargo run &

    until curl -s localhost:{{ proxy-port }} > /dev/null 2>&1; do sleep 0.5; done
    curl localhost:{{ proxy-port }}

# Run tests, lints, format checking, and spell checking to match CI
all-checks: (test '--quiet') lint fmt-check spell-check

# Run tests
test *ARGS:
    cargo test {{ everything-flags }} {{ ARGS }}

# Lint with Clippy, denying warnings
lint:
    cargo clippy {{ everything-flags }} -- --deny warnings

# Check formatting
fmt-check:
    RUSTUP_TOOLCHAIN=nightly cargo fmt --all --check && echo 'Formatting check passed'

# Check spelling with Codebook
spell-check:
    git ls-files -z | xargs -0 codebook-lsp lint
