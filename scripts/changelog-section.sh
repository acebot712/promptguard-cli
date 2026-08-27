#!/usr/bin/env bash
# Print the CHANGELOG.md section for one version, or fail if it is missing/empty.
#
#   scripts/changelog-section.sh 1.7.2 > notes.md
#
# Extracted from release-cli.yml because TWO jobs need it and they must not
# disagree: `notes-gate` runs it to refuse a release whose notes were never
# written, and `release` runs it to build the GitHub Release body. When the awk
# lived inline in the gate only, the gate carefully produced notes.md and the
# release step then published a hardcoded body -- so every release shipped
# identical, already-stale text while a gate stood guard over notes nobody read.
#
# Present-but-empty is the failure mode to catch, not just a missing heading:
# `## [1.7.2]` followed by nothing is exactly what an interrupted release
# leaves behind, and it is indistinguishable from a real section to a `grep -q`
# on the heading alone.
set -euo pipefail

if [ "$#" -ne 1 ] || [ -z "$1" ]; then
  echo "usage: $0 <version>   (e.g. 1.7.2, no leading v)" >&2
  exit 2
fi
ver="$1"
changelog="${CHANGELOG_PATH:-CHANGELOG.md}"

if [ ! -f "$changelog" ]; then
  echo "::error::$changelog not found" >&2
  exit 1
fi

section=$(awk -v v="$ver" '
  $0 ~ "^## \\[" v "\\]" { grab=1; next }
  grab && /^## \[/ { exit }
  grab { print }
' "$changelog")

if ! printf '%s' "$section" | grep -q '[^[:space:]]'; then
  echo "::error::$changelog has no non-empty '## [$ver]' section." >&2
  echo "::error::Write the release notes for $ver, then re-tag." >&2
  exit 1
fi

printf '%s\n' "$section"
