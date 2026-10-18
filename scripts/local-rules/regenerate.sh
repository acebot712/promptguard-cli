#!/usr/bin/env bash
# Regenerate the local rule pack behind `promptguard scan --offline` from a
# checkout of the source rule set (a Go module with the detectors in
# internal/security/detection).
#
#   scripts/local-rules/regenerate.sh /path/to/source-checkout
#
# Writes src/local_rules/pack.json (the rules, with a provenance block) and
# tests/local-rules-corpus.json (the source's eval samples, each with the rule
# ids the source's Go code matched on it). The source is never modified: the
# dumper, dump_test.go next to this script, runs in a scratch copy, because it
# has to sit inside the detection package to read the unexported pattern
# tables. Its import of the source's own module is written as SOURCE_MODULE
# and filled in from the checkout's go.mod.
#
# Afterwards run `cargo test --test local_rules_tests`. The parity test says
# where the Rust port and Go disagree; the pin test prints the new fingerprint
# for GENERATED_FILES_FINGERPRINT. Review the pack diff before committing it.
#
# Needs Go and rsync.
set -euo pipefail

if [[ $# -ne 1 ]]; then
    echo "usage: $0 <path-to-source-checkout>" >&2
    exit 64
fi

source_dir=$(cd "$1" && pwd)
repo=$(cd "$(dirname "$0")/../.." && pwd)
detection=internal/security/detection

if [[ ! -d "$source_dir/$detection" ]]; then
    echo "error: $source_dir has no $detection directory" >&2
    exit 1
fi
module=$(awk '$1 == "module" { print $2; exit }' "$source_dir/go.mod" 2>/dev/null || true)
if [[ -z "$module" ]]; then
    echo "error: $source_dir/go.mod names no module" >&2
    exit 1
fi

scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT

rsync -a --exclude .git --exclude node_modules --exclude dashboard --exclude sdk \
    "$source_dir/" "$scratch/source/"
sed "s#\"SOURCE_MODULE/#\"$module/#" "$repo/scripts/local-rules/dump_test.go" \
    > "$scratch/source/$detection/zz_local_rules_dump_test.go"
mkdir "$scratch/out"

(
    cd "$scratch/source"
    PACK_OUT_DIR="$scratch/out" GENERATED_ON=$(date -u +%Y-%m-%d) \
        go test "./$detection/" -run '^TestDumpOfflinePack$' -count=1
)

cp "$scratch/out/pack.json" "$repo/src/local_rules/pack.json"
cp "$scratch/out/corpus.json" "$repo/tests/local-rules-corpus.json"
echo "Wrote src/local_rules/pack.json and tests/local-rules-corpus.json."
echo "Next: cargo test --test local_rules_tests"
