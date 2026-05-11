#!/usr/bin/env bash
#
# Bump the praxis workspace version in every place it is mirrored:
#
#   - Cargo.toml          (workspace.package.version — source of truth)
#   - Cargo.lock          (regenerated via `cargo update --workspace --offline`)
#   - Dockerfile          (ARG PRAXIS_VERSION default + example comment)
#   - docker-compose.yml  (PRAXIS_VERSION default for each service)
#   - docs/src/reference/config.md  (example usage string)
#
# Usage:
#   scripts/bump-version.sh <new-version>
#
# Example:
#   scripts/bump-version.sh 0.9.27
#

set -euo pipefail

if [[ $# -ne 1 ]]; then
    echo "usage: $0 <new-version>" >&2
    exit 2
fi

NEW="$1"

if [[ ! "$NEW" =~ ^[0-9]+\.[0-9]+\.[0-9]+([-+][0-9A-Za-z.-]+)?$ ]]; then
    echo "error: '$NEW' is not a valid semver version" >&2
    exit 2
fi

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

OLD="$(grep -E '^version\s*=' Cargo.toml | head -1 | sed -E 's/.*"([^"]+)".*/\1/')"
if [[ -z "$OLD" ]]; then
    echo "error: could not read current version from Cargo.toml" >&2
    exit 1
fi

if [[ "$OLD" == "$NEW" ]]; then
    echo "version is already $NEW, nothing to do"
    exit 0
fi

echo "bumping $OLD -> $NEW"

#
# All files use simple string replacement of the full old version. We
# anchor on the previous version rather than on context lines so the
# script stays robust as surrounding text drifts.
#

sed -i "s/^version = \"$OLD\"/version = \"$NEW\"/" Cargo.toml
sed -i "s/\\\${PRAXIS_VERSION:-$OLD}/\${PRAXIS_VERSION:-$NEW}/g" docker-compose.yml
sed -i "s/PRAXIS_VERSION=$OLD/PRAXIS_VERSION=$NEW/g" Dockerfile docs/src/reference/config.md
sed -i "s/^ARG PRAXIS_VERSION=$NEW/ARG PRAXIS_VERSION=$NEW/" Dockerfile

#
# Regenerate Cargo.lock entries for workspace members. --offline avoids
# touching the registry; we only want the version fields refreshed.
#

cargo update --workspace --offline >/dev/null

echo
echo "updated files:"
git diff --stat -- Cargo.toml Cargo.lock Dockerfile docker-compose.yml docs/src/reference/config.md
