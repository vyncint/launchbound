#!/usr/bin/env bash
# Enforces the CONTRIBUTING.md commit policy over the full history:
#   - no AI/bot attribution in any commit message, author, or committer
#   - a DCO Signed-off-by trailer matching the author on every commit
set -euo pipefail

AI_PATTERN='co-authored-by:.*(claude|copilot|chatgpt|gpt|openai|anthropic|gemini|cursor|codex|devin|aider|assistant|\[bot\])'
GENERATED_PATTERN='generated (with|by).*(claude|copilot|chatgpt|gpt|openai|anthropic|gemini|cursor|codex|devin|aider)'
BOT_IDENTITY_PATTERN='(\[bot\]|noreply@anthropic\.com|noreply@openai\.com|github-actions)'

fail=0

while read -r sha; do
    msg=$(git log -1 --format='%B' "$sha")
    author_name=$(git log -1 --format='%an' "$sha")
    author_email=$(git log -1 --format='%ae' "$sha")
    committer=$(git log -1 --format='%cn <%ce>' "$sha")

    if grep -iqE "$AI_PATTERN" <<<"$msg"; then
        echo "::error::$sha: AI co-author trailer in commit message"
        fail=1
    fi
    if grep -iqE "$GENERATED_PATTERN" <<<"$msg"; then
        echo "::error::$sha: 'Generated with/by' AI watermark in commit message"
        fail=1
    fi
    if grep -q '🤖' <<<"$msg"; then
        echo "::error::$sha: robot-emoji watermark in commit message"
        fail=1
    fi
    if grep -iqE "$BOT_IDENTITY_PATTERN" <<<"$author_name <$author_email>"; then
        echo "::error::$sha: bot/vendor author identity: $author_name <$author_email>"
        fail=1
    fi
    if grep -iqE "$BOT_IDENTITY_PATTERN" <<<"$committer"; then
        echo "::error::$sha: bot/vendor committer identity: $committer"
        fail=1
    fi
    if ! grep -qF "Signed-off-by: $author_name <$author_email>" <<<"$msg"; then
        echo "::error::$sha: missing DCO Signed-off-by matching author $author_name <$author_email>"
        fail=1
    fi
done < <(git rev-list HEAD)

if [ "$fail" -ne 0 ]; then
    echo "Attribution/DCO check failed. Policy: CONTRIBUTING.md."
    exit 1
fi
echo "All $(git rev-list --count HEAD) commits clean: no AI attribution, DCO present."
