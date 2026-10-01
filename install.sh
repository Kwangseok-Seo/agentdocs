#!/bin/sh
# Install agentdocs on Linux or macOS from its GitHub releases:
#
#   curl -fsSL https://github.com/Kwangseok-Seo/agentdocs/releases/latest/download/install.sh | sh
#
# AGENTDOCS_VERSION=v0.1.0 installs that release rather than the latest, and
# AGENTDOCS_INSTALL_DIR puts the binary somewhere other than ~/.local/bin.
set -eu

repo="https://github.com/Kwangseok-Seo/agentdocs"
version="${AGENTDOCS_VERSION:-latest}"
dir="${AGENTDOCS_INSTALL_DIR:-$HOME/.local/bin}"

say() { printf 'agentdocs: %s\n' "$1"; }
fail() { printf 'agentdocs: %s\n' "$1" >&2; exit 1; }

case "$(uname -s)" in
    Linux) os=unknown-linux-musl ;;
    Darwin) os=apple-darwin ;;
    *) fail "there is no release for $(uname -s); cargo can build one: cargo install --git $repo" ;;
esac
case "$(uname -m)" in
    x86_64 | amd64) arch=x86_64 ;;
    aarch64 | arm64) arch=aarch64 ;;
    *) fail "there is no release for $(uname -m); cargo can build one: cargo install --git $repo" ;;
esac
# A shell running under Rosetta on Apple silicon is told x86_64. The Mac is
# arm64 all the same, and its own binary is the one to install.
if [ "$os" = apple-darwin ] && [ "$arch" = x86_64 ] && [ "$(sysctl -n sysctl.proc_translated 2>/dev/null)" = 1 ]; then
    arch=aarch64
fi
target="$arch-$os"
archive="agentdocs-$target.tar.gz"
if [ "$version" = latest ]; then
    from="$repo/releases/latest/download"
else
    from="$repo/releases/download/$version"
fi

if command -v curl >/dev/null 2>&1; then
    fetch() { curl -fsSL --retry 3 -o "$2" "$1"; }
elif command -v wget >/dev/null 2>&1; then
    fetch() { wget -q -O "$2" "$1"; }
else
    fail "needs curl or wget to download"
fi
if command -v sha256sum >/dev/null 2>&1; then
    sha256() { sha256sum "$1" | awk '{ print $1 }'; }
elif command -v shasum >/dev/null 2>&1; then
    sha256() { shasum -a 256 "$1" | awk '{ print $1 }'; }
else
    fail "needs sha256sum or shasum to check the download"
fi

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

say "downloading $archive ($version)"
fetch "$from/$archive" "$tmp/$archive" || fail "could not download $from/$archive"
fetch "$from/SHA256SUMS" "$tmp/SHA256SUMS" || fail "could not download $from/SHA256SUMS"

# A line of SHA256SUMS is the hash and the file's name, which some tools
# write with a `*` before it.
expected=$(awk -v name="$archive" '{ file = $2; sub(/^\*/, "", file) } file == name { print $1 }' "$tmp/SHA256SUMS")
[ -n "$expected" ] || fail "SHA256SUMS has no line for $archive"
[ "$(sha256 "$tmp/$archive")" = "$expected" ] || fail "$archive does not match its checksum; nothing was installed"

tar -xzf "$tmp/$archive" -C "$tmp"
mkdir -p "$dir"
# Copied beside the old one and moved over it, so that a failed copy leaves
# the old one as it was.
cp "$tmp/agentdocs-$target/agentdocs" "$dir/.agentdocs.new"
chmod +x "$dir/.agentdocs.new"
mv -f "$dir/.agentdocs.new" "$dir/agentdocs"

say "installed $("$dir/agentdocs" --version) as $dir/agentdocs"
case ":$PATH:" in
    *":$dir:"*) ;;
    *) say "$dir is not on your PATH; add it in your shell's profile: export PATH=\"$dir:\$PATH\"" ;;
esac
