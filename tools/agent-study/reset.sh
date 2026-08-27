#!/usr/bin/env bash
# Puts Quill and the sample project back to a known state, so one scenario cannot contaminate
# the next. Failures are ignored on purpose: a reset step with nothing to do — no modal open,
# no debug session — is not an error.
REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
Q="${QUILL_CLI:-$REPO/target/release/quill-cli.exe}"
P="${STUDY_PROJECT:-$REPO/_agent_output/agent-study/scratch-project}"
q() { "$Q" "$@" --json >/dev/null 2>&1; }

q modal cancel
q debug stop
q run stop
q pane unsplit-all
q fold expand --all
q highlight clear --all
for i in $(seq 20 -1 0); do q tab close $i --discard; done
for i in $(seq 9 -1 1); do q terminal close $i; done
q terminal hide
q explorer show
q settings set appearance.font.size 16
q editor view raw

cd "$P" || exit 1
git checkout -- . >/dev/null 2>&1
git clean -fdq -e NOTES.txt -e .quill -e target >/dev/null 2>&1
grep -q "a change nobody has committed" src/shapes.rs || echo "// a change nobody has committed" >> src/shapes.rs
[ -f NOTES.txt ] || echo "untracked" > NOTES.txt
q explorer reload
