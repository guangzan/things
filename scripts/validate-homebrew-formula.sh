#!/usr/bin/env bash
set -euo pipefail

formula="${1:?usage: validate-homebrew-formula.sh <formula>}"
tap_name="things-validation/tap"
tap_dir="$(mktemp -d)"
cleanup() {
  HOMEBREW_NO_AUTO_UPDATE=1 brew untap "$tap_name" >/dev/null 2>&1 || true
  rm -rf "$tap_dir"
}
trap cleanup EXIT

mkdir -p "$tap_dir/Formula"
cp "$formula" "$tap_dir/Formula/things.rb"
git -C "$tap_dir" init --quiet --initial-branch=main
git -C "$tap_dir" add Formula/things.rb
git -C "$tap_dir" -c user.name="things release" -c user.email="release@invalid" \
  commit --quiet -m "Validate things formula"

HOMEBREW_NO_AUTO_UPDATE=1 brew tap \
  --custom-remote "$tap_name" "file://$tap_dir" >/dev/null
if HOMEBREW_NO_AUTO_UPDATE=1 brew help trust >/dev/null 2>&1; then
  HOMEBREW_NO_AUTO_UPDATE=1 brew trust "$tap_name" >/dev/null
fi

# cargo-dist's Ruby output is valid but not fully Homebrew-style. Format it in
# tap context, then copy the exact validated contents back for publishing.
HOMEBREW_NO_AUTO_UPDATE=1 brew style --fix "$tap_name/things"
cp "$tap_dir/Formula/things.rb" "$formula"
HOMEBREW_NO_AUTO_UPDATE=1 brew style "$tap_name/things"
HOMEBREW_NO_AUTO_UPDATE=1 brew audit --strict --online "$tap_name/things"

if HOMEBREW_NO_AUTO_UPDATE=1 brew list --formula things >/dev/null 2>&1; then
  echo "Refusing to validate: things is already installed on this machine" >&2
  exit 1
fi

HOMEBREW_NO_AUTO_UPDATE=1 brew install "$tap_name/things"
HOMEBREW_NO_AUTO_UPDATE=1 brew test "$tap_name/things"
