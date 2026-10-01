#!/bin/sh
# Writes THIRD-PARTY-LICENSES.txt, the licences of what the release binaries
# are built from, which every release archive carries (docs/adr/0014): run by
# CI on every push, and by a release before anything is built. cargo-about
# reads about.toml and fills about.hbs; this runs it on x86_64 Linux.
set -eu

version=0.9.2
sha256=9099a59e820c38a68b9d65f300662a567d56562f9a10f6aa4c7e86c17c2566af
name="cargo-about-$version-x86_64-unknown-linux-musl"

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

curl -fsSL --retry 3 -o "$tmp/$name.tar.gz" \
    "https://github.com/EmbarkStudios/cargo-about/releases/download/$version/$name.tar.gz"
echo "$sha256  $tmp/$name.tar.gz" | sha256sum -c --quiet
tar -xzf "$tmp/$name.tar.gz" -C "$tmp"

# cargo-about exits 0 when it could not read a crate's licence from a file —
# a crate that ships none, or a clarification in about.toml that no longer
# applies — and gives that crate the licence's template instead, "<year>
# <copyright holders>" and all. about.hbs marks such a text, and a run that
# is right says nothing on stderr; either one fails here.
if ! "$tmp/$name/cargo-about" generate --locked -o THIRD-PARTY-LICENSES.txt about.hbs 2> "$tmp/said"; then
    cat "$tmp/said" >&2
    exit 1
fi
if [ -s "$tmp/said" ]; then
    cat "$tmp/said" >&2
    echo "::error::cargo-about warned; THIRD-PARTY-LICENSES.txt may not say what every crate says" >&2
    exit 1
fi
if grep -n 'NOT FROM A FILE' THIRD-PARTY-LICENSES.txt >&2; then
    echo "::error::a licence above was read from no file; name the crate's file in about.toml" >&2
    exit 1
fi
echo "THIRD-PARTY-LICENSES.txt: $(grep -c '^used by: ' THIRD-PARTY-LICENSES.txt) licences of crates, $(wc -l < THIRD-PARTY-LICENSES.txt) lines"
