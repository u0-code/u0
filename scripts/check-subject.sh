#!/usr/bin/env bash
# Check one commit subject, or a pull request title, against the rules in
# CONTRIBUTING.md. Pull requests are squashed, so the title of a pull request
# is the subject of the commit it becomes.
#
#   scripts/check-subject.sh "feat(work): a helper can be handed part of the work"
set -euo pipefail

subject="${1:-}"
types='feat|fix|perf|refactor|docs|style|test|bench|build|ci|chore|revert'
scopes='core|brain|net|work|mind|protocol|link|ghostd|term|telegram|discord|onebot|x|presence|bench|repo|deps|readme|assets'

fail() {
    echo "not a subject this repository takes: $subject" >&2
    echo "  $1" >&2
    echo "  the form is: type(scope): what changed, in plain words" >&2
    echo "  types:  ${types//|/, }" >&2
    echo "  scopes: ${scopes//|/, } (optional; several as \"a, b\")" >&2
    exit 1
}

[ -n "$subject" ] || fail "it is empty"
# What git and GitHub write themselves is left alone.
case "$subject" in
    "Merge "* | "Revert \""*) exit 0 ;;
esac

[[ "$subject" =~ ^($types)(\(([a-z, ]+)\))?(!)?:\ (.+)$ ]] || fail "it does not begin with one type, an optional scope, and \": \""
scope="${BASH_REMATCH[3]}"
said="${BASH_REMATCH[5]}"

if [ -n "$scope" ]; then
    IFS=',' read -ra each <<<"$scope"
    for one in "${each[@]}"; do
        one="${one# }"
        [[ "$one" =~ ^($scopes)$ ]] || fail "\"$one\" is not a scope"
    done
fi
[ "${#subject}" -le 72 ] || fail "it is ${#subject} characters long; 72 is the most"
[[ "$said" != *. ]] || fail "it ends with a full stop"
[[ "$said" != " "* ]] || fail "there is more than one space after the colon"
