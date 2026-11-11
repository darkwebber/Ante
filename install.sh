#!/bin/sh
# "Ante" installer for macOS and Linux.
#
#   curl -fsSL https://raw.githubusercontent.com/darkwebber/Ante/main/install.sh | sh
#
# Downloads a ready-made `antec` from GitHub Releases, checks its SHA-256,
# and puts it in ~/.ante/bin. Rust avasaram ledhu.
#
# Settings (environment variables):
#   ANTE_VERSION=v0.1.0       this release instead of the latest
#   ANTE_HOME=~/.ante         where to install (antec goes in $ANTE_HOME/bin)
#   ANTE_NO_MODIFY_PATH=1     don't touch your shell's startup file
#   ANTE_DIST=<url>           download from a mirror (same file names)

set -eu

REPO="darkwebber/Ante"
ANTE_HOME="${ANTE_HOME:-$HOME/.ante}"
BIN="$ANTE_HOME/bin"

say() { printf '%s\n' "$*"; }
fail() {
    printf 'error: %s\n' "$*" >&2
    exit 1
}

need() { command -v "$1" >/dev/null 2>&1 || fail "\`$1\` kavali, kaani dorakaledhu. Install chesi malli try cheyyandi"; }
need curl
need tar
need uname
need mktemp

case "$(uname -s)" in
    Darwin) os=apple-darwin ;;
    Linux) os=unknown-linux-gnu ;;
    MINGW* | MSYS* | CYGWIN*) fail "Windows lo PowerShell lo ee command run cheyyandi: irm https://raw.githubusercontent.com/$REPO/main/install.ps1 | iex" ;;
    *) fail "ee system ($(uname -s)) ki ready-made antec ledhu. Source nunchi build cheyyandi: https://github.com/$REPO#install" ;;
esac
case "$(uname -m)" in
    x86_64 | amd64) arch=x86_64 ;;
    arm64 | aarch64) arch=aarch64 ;;
    *) fail "ee CPU ($(uname -m)) ki ready-made antec ledhu. Source nunchi build cheyyandi: https://github.com/$REPO#install" ;;
esac
# An Intel build of the shell on an Apple chip still gets the native antec.
if [ "$os" = apple-darwin ] && [ "$arch" = x86_64 ] && [ "$(sysctl -n hw.optional.arm64 2>/dev/null || echo 0)" = 1 ]; then
    arch=aarch64
fi
target="$arch-$os"

if [ -n "${ANTE_VERSION:-}" ]; then
    tag="$ANTE_VERSION"
elif [ -n "${ANTE_DIST:-}" ]; then
    fail "ANTE_DIST tho paatu ANTE_VERSION kuda ivvandi"
else
    # The latest release's page redirects to its tag; no API token needed.
    latest=$(curl -fsSLo /dev/null -w '%{url_effective}' "https://github.com/$REPO/releases/latest") ||
        fail "GitHub ki connect avvaledhu. Internet check chesi malli try cheyyandi"
    tag="${latest##*/}"
    case "$tag" in v*) ;; *) fail "latest release dorakaledhu ($latest)" ;; esac
fi

name="antec-$tag-$target"
dist="${ANTE_DIST:-https://github.com/$REPO/releases/download/$tag}"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

say "📦 \"Ante\" $tag ($target) download chesthunnam..."
curl -fsSL "$dist/$name.tar.gz" -o "$tmp/$name.tar.gz" ||
    fail "$dist/$name.tar.gz download avvaledhu. Ee release lo $target ki file undha choodandi"
curl -fsSL "$dist/$name.tar.gz.sha256" -o "$tmp/$name.tar.gz.sha256" ||
    fail "checksum file download avvaledhu"

want=$(cut -d ' ' -f 1 <"$tmp/$name.tar.gz.sha256")
if command -v sha256sum >/dev/null 2>&1; then
    got=$(sha256sum "$tmp/$name.tar.gz" | cut -d ' ' -f 1)
elif command -v shasum >/dev/null 2>&1; then
    got=$(shasum -a 256 "$tmp/$name.tar.gz" | cut -d ' ' -f 1)
else
    fail "SHA-256 check cheyyadaniki sha256sum leka shasum kavali"
fi
[ "$want" = "$got" ] || fail "download paadayindhi (SHA-256 match avvaledhu). Malli try cheyyandi"

tar xzf "$tmp/$name.tar.gz" -C "$tmp"
mkdir -p "$BIN"
cp "$tmp/$name/antec" "$BIN/antec.new"
chmod +x "$BIN/antec.new"
mv -f "$BIN/antec.new" "$BIN/antec"
say "✅ antec ready: $BIN/antec"

# Put $BIN on PATH for new terminals.
case ":$PATH:" in
    *":$BIN:"*) on_path=1 ;;
    *) on_path=0 ;;
esac
if [ "$on_path" = 0 ] && [ -z "${ANTE_NO_MODIFY_PATH:-}" ]; then
    case "${SHELL:-}" in
        */zsh) rc="$HOME/.zshrc" ;;
        */bash) if [ "$os" = apple-darwin ]; then rc="$HOME/.bash_profile"; else rc="$HOME/.bashrc"; fi ;;
        */fish) rc="" ;;
        *) rc="$HOME/.profile" ;;
    esac
    line="export PATH=\"$BIN:\$PATH\""
    if [ -n "$rc" ]; then
        if ! grep -qsF "$line" "$rc"; then
            printf '\n# "Ante"\n%s\n' "$line" >>"$rc"
            say "🛣️  $rc lo PATH add chesam"
        fi
        say "   Kottha terminal open cheyyandi, leka ippude: $line"
    else
        say "🛣️  fish shell: fish_add_path $BIN"
    fi
elif [ "$on_path" = 0 ]; then
    say "🛣️  PATH lo add cheyyandi: export PATH=\"$BIN:\$PATH\""
fi

# On macOS /usr/bin/clang is there even without the Xcode tools behind it.
if [ "$os" = apple-darwin ]; then
    xcode-select -p >/dev/null 2>&1 && have_clang=1 || have_clang=0
else
    command -v clang >/dev/null 2>&1 && have_clang=1 || have_clang=0
fi
if [ "$have_clang" = 0 ]; then
    say ""
    say "ℹ️  clang ledhu: \`antec run\` interpreter lo run chesthundhi, adhi ok."
    say "   Native binaries (\`antec build\`) kosam clang install cheyyandi:"
    if [ "$os" = apple-darwin ]; then
        say "     xcode-select --install"
    else
        say "     sudo apt install clang    (Fedora: sudo dnf install clang)"
    fi
fi

say ""
"$BIN/antec" --version
say ""
say "Next:"
say "  antec new naa_project && cd naa_project && antec run"
say "  antec repl        (okko line try cheyyadaniki)"
