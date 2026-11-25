#!/usr/bin/env bash
# Search open and merged PRs (and issues) of the three repos for a keyword before building something.
# Usage: find-pr.sh <keyword> [more keywords]
q="$*"
for repo in Marvisak/VIctory fxceless/victory-fork vitutu1/victory-perf xcl-dev/xcl; do
  echo "=== $repo: $q"
  gh pr list -R "$repo" --state all --search "$q" --limit 15 --json number,title,state --jq '.[] | "#\(.number) [\(.state)] \(.title)"' 2>&1
done
