#!/usr/bin/env bash
# Creates the GitHub repo, commits, pushes, and follows the build.
# Usage: bash scripts/publish-to-github.sh [--private|--public]
set -euo pipefail

OWNER="${OWNER:-nirlotan}"
REPO="${REPO:-copilot-pacer}"
VISIBILITY="${1:---private}"
SLUG="$OWNER/$REPO"

cd "$(dirname "$0")/.."

command -v gh >/dev/null || { echo "GitHub CLI not found. Install it with: brew install gh"; exit 1; }
gh auth status >/dev/null 2>&1 || gh auth login
LOGIN="$(gh api user --jq .login)"
if [ "$LOGIN" != "$OWNER" ]; then
  echo "gh is signed in as '$LOGIN', not '$OWNER'."
  echo "Switch with:  gh auth switch -u $OWNER   (or: gh auth login)"
  exit 1
fi
gh auth setup-git   # let git push with the gh login

rm -f .github/workflows/.probe

# --- Commit -------------------------------------------------------------
[ -d .git ] || git init -q -b main
git add -A
if ! git diff --cached --quiet; then
  git commit -q -F - <<'MSG'
Copilot Pacer: menu-bar/tray app pacing GitHub Copilot credits

Tauri v2 app for macOS and Windows: tray gauge coloured by pace, popup
with usage vs. healthy pace over working days (holidays, work week and
hours configurable), GitHub device-flow / gh CLI sign-in, automatic
proxy detection (PAC/WPAD), and GitHub Actions builds for both OSes.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014LUyBBDJ95rV1k6yxRqY5Y
MSG
  echo "✓ Committed: $(git log -1 --oneline)"
fi
git branch -M main

# --- Create repo + push ----------------------------------------------------
if gh repo view "$SLUG" >/dev/null 2>&1; then
  echo "✓ Repo $SLUG already exists"
else
  gh repo create "$SLUG" "$VISIBILITY" \
    --description "Menu-bar / tray app that paces your GitHub Copilot credit usage against working days"
  echo "✓ Created $SLUG ($VISIBILITY)"
fi
git remote get-url origin >/dev/null 2>&1 || git remote add origin "https://github.com/$SLUG.git"
git push -u origin main
echo "✓ Pushed to https://github.com/$SLUG"

# --- Build -----------------------------------------------------------------
# The push to main starts the Build workflow; trigger it manually if not.
echo "Waiting for the build to start…"
RUN_ID=""
for _ in 1 2 3 4 5 6; do
  sleep 5
  RUN_ID="$(gh run list -R "$SLUG" --workflow build.yml --branch main --limit 1 --json databaseId --jq '.[0].databaseId // empty')"
  [ -n "$RUN_ID" ] && break
done
if [ -z "$RUN_ID" ]; then
  gh workflow run build.yml -R "$SLUG" --ref main
  sleep 8
  RUN_ID="$(gh run list -R "$SLUG" --workflow build.yml --limit 1 --json databaseId --jq '.[0].databaseId')"
fi
echo "✓ Build running: https://github.com/$SLUG/actions/runs/$RUN_ID"
gh run watch "$RUN_ID" -R "$SLUG" --exit-status && {
  echo "✓ Build finished. Downloading installers to ./release-artifacts …"
  gh run download "$RUN_ID" -R "$SLUG" -D release-artifacts
  ls -R release-artifacts
}
