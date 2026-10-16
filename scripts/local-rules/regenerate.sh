#!/usr/bin/env bash
# Regenerate the local rule pack behind `promptguard scan --offline` from a
# the earlier rule set checkout.
#
#   scripts/local-rules/regenerate.sh /path/to/earlier-rules
#
# Writes src/local_rules/pack.json (the rules, with a provenance block) and
# tests/local-rules-corpus.json (the earlier rule set's eval samples, each with the rule ids
# the earlier rule set's Go code matched on it). the earlier rule set is never modified: the dumper,
# dump_test.go next to this script, runs in a scratch copy, because it has to
# sit inside the earlier rule set's detection package to read the unexported pattern tables.
#
# Afterwards run `cargo test --test local_rules_tests`. The parity test says
# where the Rust port and Go disagree; the pin test prints the new fingerprint
# for PACK_FINGERPRINT. Review the pack diff before committing it.
#
# Needs Go and rsync.
set -euo pipefail

if [[ $# -ne 1 ]]; then
    echo "usage: $0 <path-to-earlier-rules-checkout>" >&2
    exit 64
fi

earlier-rules=$(cd "$1" && pwd)
repo=$(cd "$(dirname "$0")/../.." && pwd)
detection=internal/security/detection

if [[ ! -d "$earlier-rules/$detection" ]]; then
    echo "error: $earlier-rules has no $detection directory" >&2
    exit 1
fi

scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT

rsync -a --exclude .git --exclude node_modules --exclude dashboard --exclude sdk \
    "$earlier-rules/" "$scratch/earlier-rules/"
cp "$repo/scripts/local-rules/dump_test.go" "$scratch/earlier-rules/$detection/zz_local_rules_dump_test.go"
mkdir "$scratch/out"

(
    cd "$scratch/earlier-rules"
    PACK_OUT_DIR="$scratch/out" GENERATED_ON=$(date -u +%Y-%m-%d) \
        go test "./$detection/" -run '^TestDumpOfflinePack$' -count=1
)

cp "$scratch/out/pack.json" "$repo/src/local_rules/pack.json"
cp "$scratch/out/corpus.json" "$repo/tests/local-rules-corpus.json"
echo "Wrote src/local_rules/pack.json and tests/local-rules-corpus.json."
echo "Next: cargo test --test local_rules_tests"
