#!/bin/sh
# test-release.sh: proves release.sh without publishing anything. Run it with sh from anywhere:
#   sh scripts/test-release.sh
# It proves
#   - every refusal of release.sh's first step (no arguments, the wrong number of them, a bad version, a bad or short
#     commit) exits 2, prints nothing on stdout and touches nothing: it runs with an empty PATH, so release.sh could
#     not reach git or gh even if it tried, and the repository's tags and a private TMPDIR are the same afterwards
#   - --dry-run 0.2.0 <v0.2.0's commit> exits 0 and reports the tag already on origin at that commit
#   - --dry-run with a commit that is not v0.2.0's, a commit that is not on origin/main, or a version Cargo.toml does
#     not carry exits nonzero (3 for a refused commit, 4 for a tag at another commit)
#   - a dry run writes no tag, locally or on origin
# It needs the network to read (git fetch, git ls-remote, gh api); it writes nothing outside its own mktemp -d folder.
# Serves K-007 (R-2343). Prints one line "N passed, M failed" last and exits nonzero on any failure.
set -u

here=$(cd "$(dirname "$0")" && pwd)
script=$here/release.sh
repo=/srv/GitHub/knit-md-docx
user=${USER:-$(id -un)}
home=${HOME:-/}
xdg=${XDG_RUNTIME_DIR:-/run/user/$(id -u)}
good_commit=0000000000000000000000000000000000000001

work=$(mktemp -d) || { echo "test-release.sh: mktemp -d failed" >&2; exit 1; }
trap 'rm -rf -- "$work"' EXIT
trap 'exit 1' HUP INT TERM
empty=$work/empty-path
tmpd=$work/tmpdir
mkdir "$empty" "$tmpd"

pass=0
fail=0
ok() {
  pass=$((pass + 1))
  printf 'ok    %s\n' "$1"
}
bad() {
  fail=$((fail + 1))
  printf 'FAIL  %s\n' "$1"
  if [ -s "$work/err" ]; then
    sed 's/^/        /' "$work/err"
  fi
}

# What a refusal must leave as it found it: the repository's tags and the private TMPDIR.
snap() {
  git -C "$repo" for-each-ref --format='%(refname) %(objectname)' refs/tags
  ls -A "$tmpd"
}
origin_tags() { git -C "$repo" ls-remote --tags origin; }

# run_script PATH-VALUE args...: the script as his run card runs it, under env -i; stdout to out, stderr to err.
run_script() {
  _path=$1
  shift
  env -i HOME="$home" USER="$user" LOGNAME="$user" LANG=C.UTF-8 PATH="$_path" XDG_RUNTIME_DIR="$xdg" TMPDIR="$tmpd" \
    /bin/sh "$script" "$@" >"$work/out" 2>"$work/err"
}

# refuse NAME args...: exit 2, empty stdout, nothing touched.
refuse() {
  name=$1
  shift
  before=$(snap)
  rc=0
  run_script "$empty" "$@" || rc=$?
  after=$(snap)
  if [ "$rc" -eq 2 ] && [ ! -s "$work/out" ] && [ "$before" = "$after" ]; then
    ok "refuses $name"
  else
    bad "refuses $name (exit $rc, $(wc -c <"$work/out") bytes on stdout, state $([ "$before" = "$after" ] && echo same || echo CHANGED))"
  fi
}

# dry NAME WANT NEEDLE args...: a real PATH; WANT is an exit code or "nonzero"; NEEDLE (maybe empty) must be in the
# output; no tag appears locally and origin's tags stay as they were.
dry() {
  name=$1
  want=$2
  needle=$3
  shift 3
  before=$(snap)
  obefore=$(origin_tags)
  rc=0
  run_script "/usr/local/bin:/usr/bin:/bin" "$@" || rc=$?
  after=$(snap)
  oafter=$(origin_tags)
  good=1
  case $want in
    nonzero) [ "$rc" -ne 0 ] || good=0 ;;
    *) [ "$rc" -eq "$want" ] || good=0 ;;
  esac
  if [ -n "$needle" ]; then
    cat "$work/out" "$work/err" >"$work/both"
    grep -Fq -- "$needle" "$work/both" || good=0
  fi
  [ "$before" = "$after" ] || good=0
  [ "$obefore" = "$oafter" ] || good=0
  if [ "$good" -eq 1 ]; then
    ok "$name (exit $rc)"
  else
    bad "$name (exit $rc, wanted $want; local tags $([ "$before" = "$after" ] && echo same || echo CHANGED), origin tags $([ "$obefore" = "$oafter" ] && echo same || echo CHANGED))"
  fi
}

nl='
'

# ---- step 1: the refusals ----
refuse "no arguments"
refuse "one argument" 0.3.0
refuse "three arguments without --dry-run first" 0.3.0 "$good_commit" --dry-run
refuse "four arguments" 0.3.0 "$good_commit" extra more
refuse "--dry-run alone" --dry-run
refuse "--dry-run with a version only" --dry-run 0.3.0
refuse "--dry-run with four arguments" --dry-run 0.3.0 "$good_commit" extra
refuse "the version v0.3.0" v0.3.0 "$good_commit"
refuse "the version 0.3" 0.3 "$good_commit"
refuse "the version 0.3.0.1" 0.3.0.1 "$good_commit"
refuse "the version 0.3.x" 0.3.x "$good_commit"
refuse "the version 0.3.0-rc1" 0.3.0-rc1 "$good_commit"
refuse "the version 0..3" 0..3 "$good_commit"
refuse "the version .3.0" .3.0 "$good_commit"
refuse "the version 0.3." 0.3. "$good_commit"
refuse "an empty version" "" "$good_commit"
refuse "a version with a trailing newline" "0.3.0$nl" "$good_commit"
refuse "a version with two lines" "0.3.0${nl}0.3.1" "$good_commit"
refuse "a version with a space" "0.3 .0" "$good_commit"
refuse "the commit HEAD" 0.3.0 HEAD
refuse "a 7-character commit" 0.3.0 0000000
refuse "a 39-character commit" 0.3.0 000000000000000000000000000000000000001
refuse "a 41-character commit" 0.3.0 00000000000000000000000000000000000000001
refuse "an uppercase commit" 0.3.0 000000000000000000000000000000000000000A
refuse "a commit with a non-hex character" 0.3.0 000000000000000000000000000000000000000g
refuse "an empty commit" 0.3.0 ""
refuse "a commit with a trailing newline" 0.3.0 "$good_commit$nl"
refuse "--dry-run with a bad version" --dry-run 0.3 "$good_commit"
refuse "--dry-run with a short commit" --dry-run 0.3.0 abc123

# The control: well-formed arguments pass step 1, so with an empty PATH it stops at the missing tools (exit 1), not at 2.
rc=0
run_script "$empty" --dry-run 0.3.0 "$good_commit" || rc=$?
if [ "$rc" -eq 1 ] && [ ! -s "$work/out" ]; then
  ok "well-formed arguments pass step 1 (exit 1 on the missing tools)"
else
  bad "well-formed arguments pass step 1 (exit $rc, wanted 1)"
fi

# ---- steps 2 to 4 as a dry run ----
tagc=$(git -C "$repo" ls-remote origin 'refs/tags/v0.2.0^{}' 2>/dev/null | awk '{ print $1 }')
case $tagc in
  *[!0-9a-f]* | '') tagc= ;;
esac
if [ "${#tagc}" -ne 40 ]; then
  : >"$work/err"
  bad "read v0.2.0's commit from origin"
else
  dry "dry run 0.2.0 at v0.2.0's commit finds the tag on origin at that commit" 0 \
    "tag v0.2.0 is already on origin at $tagc" --dry-run 0.2.0 "$tagc"
  dry "dry run says it created and pushed nothing" 0 \
    "dry run: no tag was created and nothing was pushed" --dry-run 0.2.0 "$tagc"
  dry "dry run 0.3.0 at v0.2.0's commit: Cargo.toml says 0.2.0" 3 "Cargo.toml at $tagc says version '0.2.0'" \
    --dry-run 0.3.0 "$tagc"

  # The commits below come from origin/main as the dry runs above fetched it.
  mainref=refs/remotes/origin/main
  parent=$(git -C "$repo" rev-parse --verify --quiet "$tagc^")
  child=$(git -C "$repo" rev-list --reverse "$tagc..$mainref" | sed -n 1p)
  tip=$(git -C "$repo" rev-parse --verify --quiet "$mainref")
  if [ -n "$parent" ]; then
    dry "dry run 0.2.0 at v0.2.0's parent: Cargo.toml says 0.1.0" 3 "says version '0.1.0'" --dry-run 0.2.0 "$parent"
  else
    : >"$work/err"
    bad "find v0.2.0's parent"
  fi
  if [ -n "$child" ]; then
    dry "dry run 0.2.0 at another origin/main commit (the next one): the tag is at v0.2.0's commit" 4 \
      "origin already has v0.2.0 at $tagc" --dry-run 0.2.0 "$child"
  else
    : >"$work/err"
    bad "find a commit of origin/main after v0.2.0's"
  fi
  if [ -n "$tip" ] && [ "$tip" != "$tagc" ]; then
    dry "dry run 0.2.0 at the tip of origin/main exits nonzero" nonzero "" --dry-run 0.2.0 "$tip"
  fi
  dry "dry run 0.2.0 at a commit that does not exist" 3 "is not in $repo" \
    --dry-run 0.2.0 aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa

  # A commit in the repository that is not on origin/main: the tip of an archive branch that main never took in.
  offmain=
  for ref in $(git -C "$repo" for-each-ref --format='%(refname)' refs/remotes/origin/archive); do
    sha=$(git -C "$repo" rev-parse --verify --quiet "$ref")
    if [ -n "$sha" ] && ! git -C "$repo" merge-base --is-ancestor "$sha" "$mainref"; then
      offmain=$sha
      break
    fi
  done
  if [ -n "$offmain" ]; then
    dry "dry run 0.2.0 at a commit that is not on origin/main" 3 "is not on origin/main" --dry-run 0.2.0 "$offmain"
  else
    printf 'skip  no commit off origin/main in refs/remotes/origin/archive\n'
  fi

  # The release commit: the first commit after v0.2.0 whose Cargo.toml says 0.3.0. Before the release the dry run
  # reports the tag it would push, after it the tag already on origin; either way it writes nothing.
  relc=
  for c in $(git -C "$repo" rev-list --reverse "$tagc..$mainref"); do
    v=$(git -C "$repo" show "$c:Cargo.toml" 2>/dev/null | sed -n 's/^version = "\(.*\)"$/\1/p' | sed -n 1p)
    if [ "$v" = "0.3.0" ]; then
      relc=$c
      break
    fi
  done
  if [ -n "$relc" ]; then
    before=$(snap)
    obefore=$(origin_tags)
    rc=0
    run_script "/usr/local/bin:/usr/bin:/bin" --dry-run 0.3.0 "$relc" || rc=$?
    after=$(snap)
    oafter=$(origin_tags)
    cat "$work/out" "$work/err" >"$work/both"
    if [ "$rc" -eq 0 ] && [ "$before" = "$after" ] && [ "$obefore" = "$oafter" ] \
      && { grep -Fq -- "would push refs/tags/v0.3.0 to origin" "$work/both" \
        || grep -Fq -- "tag v0.3.0 is already on origin at $relc" "$work/both"; }; then
      ok "dry run 0.3.0 at the release commit writes no tag and pushes none (exit $rc)"
    else
      bad "dry run 0.3.0 at the release commit (exit $rc, local tags $([ "$before" = "$after" ] && echo same || echo CHANGED), origin tags $([ "$obefore" = "$oafter" ] && echo same || echo CHANGED))"
    fi
  else
    printf 'skip  no commit of origin/main after v0.2.0 says version 0.3.0\n'
  fi
fi

printf '%s passed, %s failed\n' "$pass" "$fail"
[ "$fail" -eq 0 ]
