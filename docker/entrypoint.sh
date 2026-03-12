#!/bin/bash
set -euo pipefail

echo "[entrypoint] Starting Claude Code pod"
echo "[entrypoint] Claude version: $(claude --version)"
echo "[entrypoint] Project directory: /workspace"
echo "[entrypoint] Mode: ${CLAUDE_MODE:-daemon}"

# Set up writable ~/.claude directory.
# /claude-data is an emptyDir (writable). /claude-host is the host's ~/.claude (read-only).
# We symlink ~/.claude → /claude-data so any base image user (node, claude, etc.) works.
CLAUDE_DIR="$HOME/.claude"
if [ -d /claude-data ]; then
    # Remove any existing ~/.claude (may be a dir from the Docker image build)
    rm -rf "$CLAUDE_DIR" 2>/dev/null || true
    ln -sf /claude-data "$CLAUDE_DIR"
    echo "[entrypoint] Linked $CLAUDE_DIR -> /claude-data (writable)"
fi

# Copy host credentials into the writable dir
if [ -d /claude-host ]; then
    echo "[entrypoint] Copying credentials from host mount"
    for f in .credentials.json settings.json settings.local.json; do
        [ -f "/claude-host/$f" ] && cp "/claude-host/$f" /claude-data/"$f" 2>/dev/null || true
    done
    for d in statsig; do
        [ -d "/claude-host/$d" ] && cp -r "/claude-host/$d" /claude-data/"$d" 2>/dev/null || true
    done
fi

# Configure git identity if provided
if [ -n "${GIT_USER_NAME:-}" ]; then
    git config --global user.name "$GIT_USER_NAME"
fi
if [ -n "${GIT_USER_EMAIL:-}" ]; then
    git config --global user.email "$GIT_USER_EMAIL"
fi

# Trust the workspace directory for git
git config --global --add safe.directory /workspace

# Pre-trust the /workspace directory so Claude doesn't prompt
CLAUDE_JSON="$HOME/.claude.json"
if [ -f "$CLAUDE_JSON" ]; then
    # Add /workspace trust entry if not already present
    if command -v python3 >/dev/null 2>&1; then
        python3 -c "
import json, sys
try:
    with open('$CLAUDE_JSON') as f:
        data = json.load(f)
except:
    data = {}
projects = data.setdefault('projects', {})
if '/workspace' not in projects:
    projects['/workspace'] = {'allowedTools': [], 'hasTrustDialogAccepted': True}
elif not projects['/workspace'].get('hasTrustDialogAccepted'):
    projects['/workspace']['hasTrustDialogAccepted'] = True
else:
    sys.exit(0)
with open('$CLAUDE_JSON', 'w') as f:
    json.dump(data, f, indent=2)
" 2>/dev/null || true
    elif command -v jq >/dev/null 2>&1; then
        tmp=$(mktemp)
        jq '.projects["/workspace"].hasTrustDialogAccepted = true' "$CLAUDE_JSON" > "$tmp" && mv "$tmp" "$CLAUDE_JSON" 2>/dev/null || true
    fi
elif [ -d "$(dirname "$CLAUDE_JSON")" ]; then
    echo '{"projects":{"/workspace":{"allowedTools":[],"hasTrustDialogAccepted":true}}}' > "$CLAUDE_JSON" 2>/dev/null || true
fi

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
