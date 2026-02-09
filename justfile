proxy-port := '3000'
backend-port := '8000'

# Run a simple demo of the proxying behavior
demo:
    #!/usr/bin/env bash
    trap 'kill $(jobs -p)' EXIT

    printf '\nRunning Python backend on port {{ backend-port }}\n\n'
    python3 -m http.server {{ backend-port }} &

    printf 'Running reverse proxy on port {{ proxy-port }}\n\n'
    cargo run &

    until curl -s localhost:{{ proxy-port }} > /dev/null 2>&1; do sleep 0.5; done
    curl localhost:{{ proxy-port }}
