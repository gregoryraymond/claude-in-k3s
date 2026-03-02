#!/bin/bash
set -euo pipefail

echo "[entrypoint] Starting Claude Code pod"
echo "[entrypoint] Claude version: $(claude --version)"
echo "[entrypoint] Project directory: /workspace"
echo "[entrypoint] Mode: ${CLAUDE_MODE:-daemon}"

# Configure git identity if provided
if [ -n "${GIT_USER_NAME:-}" ]; then
    git config --global user.name "$GIT_USER_NAME"
fi
if [ -n "${GIT_USER_EMAIL:-}" ]; then
    git config --global user.email "$GIT_USER_EMAIL"
fi

# Trust the workspace directory for git
git config --global --add safe.directory /workspace

CLAUDE_MODE="${CLAUDE_MODE:-daemon}"

case "$CLAUDE_MODE" in
    headless)
        # Run a prompt and exit
        PROMPT="${CLAUDE_PROMPT:-Analyze this project and provide a summary.}"
        exec claude -p \
            --dangerously-skip-permissions \
            --verbose \
            --output-format stream-json \
            -- "$PROMPT"
        ;;
    daemon)
        # Keep the pod alive for repeated kubectl exec invocations
        echo "[entrypoint] Running in daemon mode - pod stays alive"
        echo "[entrypoint] Use 'kubectl exec' to send commands"
        exec tail -f /dev/null
        ;;
    *)
        echo "[entrypoint] Unknown mode: $CLAUDE_MODE"
        exit 1
        ;;
esac
