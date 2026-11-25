#!/usr/bin/env bash
# Show what changed upstream since the last check, and new open PRs.
# Usage: check-upstream.sh [since-date YYYY-MM-DD]   (default: 2 days ago)
since="${1:-$(date -d '2 days ago' +%F)}"
for repo in Marvisak/VIctory fxceless/victory-fork vitutu1/victory-perf xcl-dev/xcl; do
  echo "=== $repo: commits on default branch since $since"
  gh api "repos/$repo/commits?since=${since}T00:00:00Z&per_page=30" --jq '.[] | "\(.sha[0:7]) \(.commit.message|split("\n")[0])"' 2>&1 | head -30
  echo "--- merged PRs since $since"
  gh pr list -R "$repo" --state merged --search "merged:>=$since" --limit 30 --json number,title --jq '.[] | "#\(.number) \(.title)"' 2>&1
  echo "--- new open PRs since $since"
  gh pr list -R "$repo" --state open --search "created:>=$since" --limit 30 --json number,title,author --jq '.[] | "#\(.number) \(.title) (\(.author.login))"' 2>&1
done
