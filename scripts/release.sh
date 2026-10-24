#!/bin/sh
# release.sh: publish a knit-md-docx release. It tags <commit> as v<version> in the one repository below, pushes
# that tag, waits for the Release workflow (.github/workflows/release.yml) the tag starts, then checks what the
# workflow published, so that terminal-config's pins.conf can be moved to the checksums it prints last.
# Serves K-007 (release v0.3.0 so installed machines get the themes, R-0932). Publishing is his run card (R-2343):
#   run /bin/sh /srv/GitHub/knit-md-docx/scripts/release.sh <version> <commit> | file <sha256> <path>
# so it relies on no variable, profile or ssh agent: gh's login in ~/.config/gh and git's credential helper for https.
#
# Usage:
#   sh release.sh <version> <commit>             tag, push, wait, verify
#   sh release.sh --dry-run <version> <commit>   the checks only; no tag, no push, no temp folder
#     <version>  X.Y.Z, digits only, equal to Cargo.toml's package version at <commit>
#     <commit>   40 lowercase hex characters, on origin/main of the repository below
#
# Steps, in order:
#   1. refuse a <version> or <commit> of the wrong shape, before anything else is read or run
#   2. fetch origin; <commit> must be on origin/main; Cargo.toml at <commit> must say <version>; DOCX_RS_REV at
#      <commit> must name a commit on main of danganhtu01/knit-md-docx-rs (gh api compare: ahead or identical)
#   3. the tag: if origin has v<version>, it must peel to <commit> and the push is skipped (a rerun is idempotent);
#      else a local v<version> must point at <commit> or be absent, and a lightweight tag is created and pushed.
#      It never moves, deletes or forces a tag, and never pushes a branch or --tags.
#   4. find the Release run of that tag at <commit> (polled every 15 s for 5 minutes), follow it to its end
#      (at most 2700 s); a run that fails or times out exits with its URL
#   5. download the release; both assets and SHA256SUMS must be there and sha256sum -c must pass; print last
#        version <version>
#        linux <sha256 of knit-md-docx-x86_64-unknown-linux-musl.tar.gz>
#        windows <sha256 of knit-md-docx-x86_64-pc-windows-msvc.zip>
#
# Writes only (C-60): the tag v<version> of the repository below, its remote-tracking refs by git fetch, and a
# mktemp -d folder of its own, removed by a trap. The repository is a fixed path; no variable changes it.
#
# Exit codes:
#   0  done (with --dry-run: every check passed)
#   1  a tool is missing, or git, gh or the network failed
#   2  wrong arguments, a <version> not X.Y.Z, or a <commit> not 40 lowercase hex; nothing was touched
#   3  <commit> refused: not on origin/main, a Cargo.toml version other than <version>, or a DOCX_RS_REV not on
#      main of danganhtu01/knit-md-docx-rs
#   4  a tag conflict: origin or the local repository has v<version> at another commit (never moved)
#   5  pushing the tag failed
#   6  the Release run did not appear, did not succeed, or did not finish in time
#   7  the published release failed its checks
set -eu

repo=/srv/GitHub/knit-md-docx
slug=danganhtu01/knit-md-docx
fork=danganhtu01/knit-md-docx-rs
linux_asset=knit-md-docx-x86_64-unknown-linux-musl.tar.gz
windows_asset=knit-md-docx-x86_64-pc-windows-msvc.zip

# Nothing the caller left in the environment may point git or gh somewhere else.
unset GIT_DIR GIT_WORK_TREE GIT_INDEX_FILE GIT_COMMON_DIR GIT_OBJECT_DIRECTORY GIT_NAMESPACE GH_REPO GH_HOST
GIT_TERMINAL_PROMPT=0
GH_PROMPT_DISABLED=1
GH_NO_UPDATE_NOTIFIER=1
GH_NO_EXTENSION_UPDATE_NOTIFIER=1
export GIT_TERMINAL_PROMPT GH_PROMPT_DISABLED GH_NO_UPDATE_NOTIFIER GH_NO_EXTENSION_UPDATE_NOTIFIER

say() { printf '%s\n' "$*"; }
die() {
  code=$1
  shift
  printf 'release.sh: %s\n' "$*" >&2
  exit "$code"
}
usage() {
  printf 'usage: sh release.sh [--dry-run] <version X.Y.Z> <commit, 40 lowercase hex>\n' >&2
  exit 2
}
g() { git -C "$repo" "$@"; }

# ---- 1. the arguments ----
dry=0
if [ "$#" -ge 1 ] && [ "$1" = "--dry-run" ]; then
  dry=1
  shift
fi
[ "$#" -eq 2 ] || usage
version=$1
commit=$2

is_digits() {
  case $1 in
    '' | *[!0-9]*) return 1 ;;
  esac
  return 0
}
valid_version() {
  case $1 in
    *.*.*) ;;
    *) return 1 ;;
  esac
  _a=${1%%.*}
  _rest=${1#*.}
  case $_rest in
    *.*) ;;
    *) return 1 ;;
  esac
  _b=${_rest%%.*}
  _c=${_rest#*.}
  is_digits "$_a" && is_digits "$_b" && is_digits "$_c"
}
valid_commit() {
  case $1 in
    *[!0-9a-f]*) return 1 ;;
  esac
  [ "${#1}" -eq 40 ]
}
valid_version "$version" || { printf 'release.sh: the version must be X.Y.Z, digits only\n' >&2; exit 2; }
valid_commit "$commit" || { printf 'release.sh: the commit must be 40 lowercase hex characters\n' >&2; exit 2; }
tag=v$version
tagref=refs/tags/$tag

for tool in git gh awk sed tr sha256sum mktemp timeout sleep; do
  command -v "$tool" >/dev/null 2>&1 || die 1 "$tool is not on PATH"
done

# ---- 2. the repository, the commit, Cargo.toml and DOCX_RS_REV ----
if [ "$dry" -eq 1 ]; then
  say "dry run: nothing is tagged or pushed"
fi
say "release $tag at $commit in $repo"
origin_url=$(g remote get-url origin 2>/dev/null) || die 1 "$repo is not a git repository with an origin"
case $origin_url in
  "https://github.com/$slug" | "https://github.com/$slug.git") ;;
  *) die 1 "origin of $repo is $origin_url, not https://github.com/$slug" ;;
esac
g fetch --quiet --no-tags origin || die 1 "git fetch origin failed in $repo"
mainref=refs/remotes/origin/main
g rev-parse --verify --quiet "$mainref^{commit}" >/dev/null || die 3 "origin/main is not known in $repo after the fetch"
g cat-file -e "$commit^{commit}" 2>/dev/null || die 3 "commit $commit is not in $repo after the fetch, so it is not on origin/main"
g merge-base --is-ancestor "$commit" "$mainref" || die 3 "commit $commit is not on origin/main"
say "the commit is on origin/main"

cargo=$(g show "$commit:Cargo.toml" 2>/dev/null) || die 3 "there is no Cargo.toml at $commit"
# The package version as Cargo reads it, and as release.yml reads it; both must say <version>.
pkg_version=$(printf '%s\n' "$cargo" | awk '
  /^\[/ { inpkg = ($0 ~ /^\[package\][[:space:]]*$/); next }
  inpkg && $0 ~ /^version[[:space:]]*=[[:space:]]*"[^"]*"[[:space:]]*$/ {
    v = $0
    sub(/^version[[:space:]]*=[[:space:]]*"/, "", v)
    sub(/".*$/, "", v)
    print v
    exit
  }')
flow_version=$(printf '%s\n' "$cargo" | sed -n 's/^version = "\(.*\)"$/\1/p' | sed -n 1p)
[ "$pkg_version" = "$version" ] || die 3 "Cargo.toml at $commit says version '$pkg_version', not '$version'"
[ "$flow_version" = "$version" ] || die 3 "release.yml would read version '$flow_version' from Cargo.toml at $commit, not '$version'"
say "Cargo.toml at the commit says version $version"

rev_file=$(g show "$commit:DOCX_RS_REV" 2>/dev/null) || die 3 "there is no DOCX_RS_REV at $commit"
rev=$(printf '%s' "$rev_file" | tr -d '[:space:]')
valid_commit "$rev" || die 3 "DOCX_RS_REV at $commit is not 40 lowercase hex characters"
if ! cmp_out=$(gh api "repos/$fork/compare/$rev...main" --jq .status 2>&1); then
  case $cmp_out in
    *"HTTP 404"*) die 3 "DOCX_RS_REV $rev is not a commit of $fork on GitHub" ;;
    *) die 1 "gh api compare failed: $cmp_out" ;;
  esac
fi
case $cmp_out in
  ahead | identical) ;;
  *) die 3 "DOCX_RS_REV $rev is not on main of $fork (compare status '$cmp_out')" ;;
esac
say "DOCX_RS_REV $rev is on main of $fork (compare status $cmp_out)"

# ---- 3. the tag ----
remote_tags=$(g ls-remote origin "$tagref" "$tagref^{}") || die 1 "git ls-remote origin failed in $repo"
origin_obj=$(printf '%s\n' "$remote_tags" | awk -v r="$tagref" '$2 == r { print $1 }')
origin_peeled=$(printf '%s\n' "$remote_tags" | awk -v r="$tagref^{}" '$2 == r { print $1 }')
push_needed=1
have_local=0
if [ -n "$origin_obj" ]; then
  # An annotated tag lists its commit on the ^{} line; a lightweight tag is the commit itself.
  origin_commit=${origin_peeled:-$origin_obj}
  [ "$origin_commit" = "$commit" ] || die 4 "origin already has $tag at $origin_commit, not $commit; this script never moves or deletes a tag"
  push_needed=0
  say "tag $tag is already on origin at $commit; nothing to push"
else
  if g show-ref --verify --quiet "$tagref"; then
    local_commit=$(g rev-parse --verify --quiet "$tagref^{commit}") || local_commit=
    [ "$local_commit" = "$commit" ] || die 4 "the local tag $tag points at '$local_commit', not $commit; this script never moves or deletes a tag"
    have_local=1
    say "the local tag $tag already points at $commit"
  fi
  if [ "$dry" -eq 1 ]; then
    [ "$have_local" -eq 1 ] || say "would create the lightweight tag $tag at $commit"
    say "would push $tagref to origin, which starts the Release workflow"
  fi
fi

if [ "$dry" -eq 1 ]; then
  say "dry run: no tag was created and nothing was pushed"
  exit 0
fi

# ---- the temp folder, removed on any exit ----
tmp=$(mktemp -d) || die 1 "mktemp -d failed"
cleanup() {
  if [ -n "$tmp" ]; then
    rm -rf -- "$tmp"
  fi
}
trap cleanup EXIT
trap 'exit 1' HUP INT TERM

if [ "$push_needed" -eq 1 ]; then
  if [ "$have_local" -eq 0 ]; then
    # tag.gpgSign may be set globally; a lightweight tag is wanted, so it is switched off for this one call.
    g -c tag.gpgSign=false tag "$tag" "$commit" || die 1 "git tag $tag $commit failed"
    say "created the lightweight tag $tag at $commit"
  fi
  g push origin "$tagref:$tagref" || die 5 "git push origin $tagref failed"
  say "pushed $tagref to origin"
fi

# ---- 4. the Release run ----
polls=0
run_id=
while :; do
  if list_out=$(gh run list -R "$slug" --workflow release.yml --limit 50 \
    --json databaseId,headSha,status,conclusion,headBranch \
    --jq ".[] | select(.headSha == \"$commit\" and .headBranch == \"$tag\") | .databaseId" 2>&1); then
    run_id=$(printf '%s\n' "$list_out" | sed -n 1p)
    if [ -n "$run_id" ]; then
      break
    fi
    say "no Release run of $tag at $commit yet (poll $polls of 20)"
  else
    say "gh run list failed (poll $polls of 20): $(printf '%s\n' "$list_out" | sed -n 1p)"
  fi
  [ "$polls" -lt 20 ] || die 6 "no Release run of $tag at $commit appeared within 5 minutes: https://github.com/$slug/actions"
  polls=$((polls + 1))
  sleep 15
done
is_digits "$run_id" || die 6 "gh run list gave '$run_id' for the run id"
run_url=https://github.com/$slug/actions/runs/$run_id
say "following the Release run $run_id: $run_url"
if timeout 2700 gh run watch "$run_id" -R "$slug" --exit-status --interval 30; then
  :
else
  watch_rc=$?
  if [ "$watch_rc" -eq 124 ]; then
    die 6 "the Release run $run_id did not finish within 2700 s: $run_url"
  fi
  die 6 "the Release run $run_id failed (exit $watch_rc): $run_url"
fi
run_state=$(gh run view "$run_id" -R "$slug" --json status,conclusion --jq '.status + " " + .conclusion') || die 6 "gh run view $run_id failed: $run_url"
[ "$run_state" = "completed success" ] || die 6 "the Release run $run_id ended '$run_state': $run_url"
say "the Release run $run_id succeeded"

# ---- 5. the published release ----
draft=$(gh release view "$tag" -R "$slug" --json isDraft --jq .isDraft) || die 7 "gh release view $tag failed"
[ "$draft" = "false" ] || die 7 "the release $tag is a draft"
dl=$tmp/download
mkdir "$dl"
gh release download "$tag" -R "$slug" --dir "$dl" || die 7 "gh release download $tag failed"
for f in "$linux_asset" "$windows_asset" SHA256SUMS; do
  [ -s "$dl/$f" ] || die 7 "the release $tag has no $f"
done
for f in "$linux_asset" "$windows_asset"; do
  awk -v f="$f" '$2 == f || $2 == "*" f { n++ } END { exit (n == 1 ? 0 : 1) }' "$dl/SHA256SUMS" || die 7 "SHA256SUMS does not list $f exactly once"
done
(cd "$dl" && sha256sum -c SHA256SUMS) || die 7 "sha256sum -c SHA256SUMS failed for the release $tag"
linux_sha=$(sha256sum "$dl/$linux_asset" | awk '{ print $1 }')
windows_sha=$(sha256sum "$dl/$windows_asset" | awk '{ print $1 }')
valid_sha() { case $1 in *[!0-9a-f]*) return 1 ;; esac; [ "${#1}" -eq 64 ]; }
valid_sha "$linux_sha" || die 7 "no sha256 for $linux_asset"
valid_sha "$windows_sha" || die 7 "no sha256 for $windows_asset"
say "the release $tag has both assets and SHA256SUMS, and the checksums match"
say "version $version"
say "linux $linux_sha"
say "windows $windows_sha"
