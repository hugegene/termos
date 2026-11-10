#!/bin/sh
# Packs a built tb for a GitHub release: dist/tb-<target>.tar.gz and dist/tb-<target>.tar.gz.sha256.
# The tarball holds one folder, tb-<version>-<target>/, with tb, LICENSE, THIRD_PARTY_NOTICES.md and README.md.
#
#   scripts/package.sh <target> <version> [path to tb]
#
# Needs THIRD_PARTY_NOTICES.md in the repo root: cargo about generate about.hbs -o THIRD_PARTY_NOTICES.md
# The release workflow (.github/workflows/release.yml) runs this; it also works by hand.
set -eu

[ $# -ge 2 ] || {
    echo "usage: $0 <target> <version> [path to tb]" >&2
    exit 2
}
target=$1
version=${2#v}
cd "$(dirname "$0")/.."
bin=${3:-target/$target/release/tb}

# The tag, Cargo.toml and the binary must agree, and the binary must run here.
said=$("$bin" --version)
case "$said" in
"tb $version" | "tb $version "*) ;;
*)
    echo "package.sh: $bin --version says '$said', expected tb $version" >&2
    exit 1
    ;;
esac
[ -f THIRD_PARTY_NOTICES.md ] || {
    echo "package.sh: THIRD_PARTY_NOTICES.md is missing; run: cargo about generate about.hbs -o THIRD_PARTY_NOTICES.md" >&2
    exit 1
}

name=tb-$version-$target
asset=tb-$target.tar.gz
rm -rf "dist/$name" "dist/$asset" "dist/$asset.sha256"
mkdir -p "dist/$name"
cp "$bin" "dist/$name/tb"
cp LICENSE THIRD_PARTY_NOTICES.md "dist/$name/"
cp scripts/release-README.md "dist/$name/README.md"
# COPYFILE_DISABLE keeps macOS's tar from adding ._ metadata files.
COPYFILE_DISABLE=1 tar -C dist -czf "dist/$asset" "$name"
cd dist
if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$asset" >"$asset.sha256"
else
    shasum -a 256 "$asset" >"$asset.sha256"
fi
echo "dist/$asset"
