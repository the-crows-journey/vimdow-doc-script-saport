#!/usr/bin/env bash
# build.sh - one-command build of the complete VimDow Godot addon.
#
# Usage:   ./build.sh [--profile debug|release|both] [--out DIR] [--help]
#
# What it does (mirrors `inv export_plugin` from tasks.py, without touching
# the source tree):
#   1. Checks the toolchain (cargo/rustc, supported host platform).
#   2. Builds the Rust GDExtension with `cargo build --manifest-path
#      rust/Cargo.toml` for each profile (default: debug AND release, because
#      vimdow.gdextension references both bin/debug and bin/release).
#   3. Stages godot/addons/vimdow (minus dev-only files and any stale bin/) in
#      a temporary directory and adds the freshly built libraries.
#   4. Verifies the staged addon (plugin script, gdextension library paths,
#      library files, entry symbol).
#   5. Only if everything succeeded, publishes it to
#      $HOME/Downloads/vimdow-addon/addons/vimdow
#
# Nothing in the source project is modified or deleted. Cargo output goes to
# the usual (git-ignored) rust/target directory.

set -Eeuo pipefail

# ---------------------------------------------------------------- settings --
PROJECT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ADDON_SRC="$PROJECT_DIR/godot/addons/vimdow"
CARGO_MANIFEST="$PROJECT_DIR/rust/Cargo.toml"
LIB_BASENAME="vimdow"                       # crate name -> libvimdow.so
PROFILE="both"
OUT_ROOT="${VIMDOW_OUT_DIR:-$HOME/Downloads}"
OUT_NAME="vimdow-addon"                      # directory created in OUT_ROOT
MARKER=".vimdow-build-output"                # lets us safely replace our own output only

# ----------------------------------------------------------------- helpers --
if [ -t 1 ]; then B=$'\033[1m'; R=$'\033[31m'; G=$'\033[32m'; Y=$'\033[33m'; N=$'\033[0m'; else B=; R=; G=; Y=; N=; fi
step() { printf '\n%s==> %s%s\n' "$B" "$*" "$N"; }
info() { printf '    %s\n' "$*"; }
warn() { printf '%sWarning:%s %s\n' "$Y" "$N" "$*" >&2; }
die()  { printf '\n%sERROR:%s %s\n' "$R" "$N" "$*" >&2; printf '%sBuild FAILED. Nothing was packaged or published.%s\n' "$R" "$N" >&2; exit 1; }

STAGE=""
cleanup() { [ -n "$STAGE" ] && [ -d "$STAGE" ] && rm -rf -- "$STAGE"; return 0; }
trap cleanup EXIT
trap 'die "Unexpected failure at line $LINENO: $BASH_COMMAND"' ERR

usage() {
  cat <<EOF
Usage: ./build.sh [options]

Builds the complete VimDow Godot addon and writes it to
  \$HOME/Downloads/$OUT_NAME/addons/vimdow

Options:
  --profile P   debug | release | both   (default: both)
  --out DIR     output root instead of \$HOME/Downloads
  -h, --help    show this help

Environment:
  VIMDOW_OUT_DIR   same as --out
  CARGO_TARGET_DIR respected if set (default: rust/target)
EOF
}

while [ $# -gt 0 ]; do
  case "$1" in
    --profile) [ $# -ge 2 ] || die "--profile needs a value"; PROFILE="$2"; shift 2 ;;
    --out)     [ $# -ge 2 ] || die "--out needs a value"; OUT_ROOT="$2"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) usage >&2; die "Unknown option: $1" ;;
  esac
done

case "$PROFILE" in
  debug)   PROFILES=(debug) ;;
  release) PROFILES=(release) ;;
  both)    PROFILES=(debug release) ;;
  *) die "Invalid --profile '$PROFILE' (use debug, release or both)" ;;
esac

# --------------------------------------------------------------- preflight --
step "Checking project and toolchain"
[ -f "$CARGO_MANIFEST" ]            || die "Missing $CARGO_MANIFEST - run this script from a VimDow checkout."
[ -f "$ADDON_SRC/plugin.cfg" ]      || die "Missing $ADDON_SRC/plugin.cfg"
[ -f "$ADDON_SRC/vimdow.gdextension" ] || die "Missing $ADDON_SRC/vimdow.gdextension"
command -v cargo >/dev/null 2>&1 || die "'cargo' not found in PATH. Install Rust (https://rustup.rs) and re-run."
command -v rustc >/dev/null 2>&1 || die "'rustc' not found in PATH. Install Rust (https://rustup.rs) and re-run."
info "$(rustc --version) / $(cargo --version)"

OS="$(uname -s)"; ARCH="$(uname -m)"
case "$OS/$ARCH" in
  Linux/x86_64)  LIBFILE="lib${LIB_BASENAME}.so" ; PLATFORM_DESC="linux x86_64" ; TARGET_TRIPLE="x86_64-unknown-linux-gnu" ;;
  Darwin/*)      LIBFILE="lib${LIB_BASENAME}.dylib"; PLATFORM_DESC="macos $ARCH"; TARGET_TRIPLE="native-macos" ;;
  *) die "Unsupported host platform $OS/$ARCH. vimdow.gdextension only declares linux x86_64, windows x86_64 and macos." ;;
esac
info "Host platform: $PLATFORM_DESC -> $LIBFILE"
info "Profiles:      ${PROFILES[*]}"

CARGO_TARGET="${CARGO_TARGET_DIR:-$PROJECT_DIR/rust/target}"

# ------------------------------------------------------------ rust compile --
for p in "${PROFILES[@]}"; do
  step "Compiling Rust GDExtension ($p)"
  flags=(--locked --manifest-path "$CARGO_MANIFEST")
  [ "$p" = "release" ] && flags+=(--release)
  if ! cargo build "${flags[@]}"; then
    die "cargo build ($p) failed - see the compiler output above. If the message mentions Cargo.lock, the lockfile is out of date; this script uses --locked so it never edits Cargo.lock."
  fi
  [ -f "$CARGO_TARGET/$p/$LIBFILE" ] || die "cargo reported success but $CARGO_TARGET/$p/$LIBFILE does not exist (is a custom build.target or target-dir configured in your cargo config?)."
  info "Built $CARGO_TARGET/$p/$LIBFILE"
done

# ----------------------------------------------------------------- staging --
step "Staging addon files"
STAGE="$(mktemp -d "${TMPDIR:-/tmp}/vimdow-build.XXXXXX")"
STAGED="$STAGE/$OUT_NAME/addons/vimdow"
mkdir -p "$STAGED"

# Runtime files only: drop bin/ (stale local builds are replaced by fresh ones),
# dev-only formatter config, and editor/OS junk.
if command -v rsync >/dev/null 2>&1; then
  rsync -a --exclude '/bin/' --exclude '.stylua.toml' --exclude '.DS_Store' --exclude '*.tmp' --exclude '*~' "$ADDON_SRC/" "$STAGED/"
else
  (cd "$ADDON_SRC" && find . -type f ! -path './bin/*' ! -name '.stylua.toml' ! -name '.DS_Store' ! -name '*.tmp' ! -name '*~' -print0 \
     | while IFS= read -r -d '' f; do mkdir -p "$STAGED/$(dirname "$f")"; cp -p "$f" "$STAGED/$f"; done)
fi
[ -f "$PROJECT_DIR/README.md" ] && cp "$PROJECT_DIR/README.md" "$STAGED/README.md"
[ -f "$PROJECT_DIR/LICENSE" ]   && cp "$PROJECT_DIR/LICENSE"   "$STAGED/LICENSE"

# Same default as `inv export_plugin`: ship with the built-in neovim template config on.
# (applied to the staged copy only; the source local.cfg is untouched)
if [ -f "$STAGED/local.cfg" ]; then
  sed -i -E 's/^([[:space:]]*template[[:space:]]*=).*/\1 true/' "$STAGED/local.cfg"
fi

for p in "${PROFILES[@]}"; do
  mkdir -p "$STAGED/bin/$p"
  cp "$CARGO_TARGET/$p/$LIBFILE" "$STAGED/bin/$p/$LIBFILE"
  info "bin/$p/$LIBFILE"
done

# ------------------------------------------------------------ verification --
step "Verifying staged addon"
script="$(sed -n -E 's/^[[:space:]]*script[[:space:]]*=[[:space:]]*"([^"]+)".*/\1/p' "$STAGED/plugin.cfg" | head -n1)"
[ -n "$script" ] && [ -f "$STAGED/$script" ] || die "plugin.cfg script '$script' not found in packaged addon."
info "plugin script OK ($script)"

# every res://addons/vimdow/... library referenced for THIS platform/profile must exist
os_key="linux"; [ "$OS" = "Darwin" ] && os_key="macos"
for p in "${PROFILES[@]}"; do
  rel="$(sed -n -E "s#^[[:space:]]*${os_key}\.${p}[A-Za-z0-9_.]*[[:space:]]*=[[:space:]]*\"res://addons/vimdow/([^\"]+)\".*#\1#p" "$STAGED/vimdow.gdextension" | head -n1)"
  [ -n "$rel" ] || die "vimdow.gdextension has no $os_key.$p library entry."
  [ -f "$STAGED/$rel" ] || die "vimdow.gdextension references '$rel' but it is missing from the package."
  info "gdextension $os_key.$p -> $rel OK"
  if command -v file >/dev/null 2>&1; then
    file -b "$STAGED/$rel" | grep -qiE 'ELF|Mach-O|shared object|dynamically linked' \
      || die "'$rel' is not a valid shared library: $(file -b "$STAGED/$rel")"
  fi
  if [ "$OS" = "Linux" ] && command -v nm >/dev/null 2>&1; then
    nm -D --defined-only "$STAGED/$rel" 2>/dev/null | grep -q ' gdext_rust_init$' \
      || die "'$rel' does not export the gdext_rust_init entry symbol."
    info "entry symbol gdext_rust_init OK ($p)"
  fi
done
for f in vimdow.gdextension plugin.cfg local.cfg; do [ -f "$STAGED/$f" ] || die "Package is missing $f"; done

# --------------------------------------------------------------- publishing --
step "Publishing to $OUT_ROOT/$OUT_NAME"
mkdir -p "$OUT_ROOT" || die "Cannot create output directory $OUT_ROOT"
FINAL="$OUT_ROOT/$OUT_NAME"
if [ -e "$FINAL" ]; then
  [ -f "$FINAL/$MARKER" ] || die "$FINAL already exists and was not created by this script; refusing to overwrite it. Move it away or use --out."
  rm -rf -- "$FINAL"      # our own previous output only
fi
touch "$STAGE/$OUT_NAME/$MARKER"
mv "$STAGE/$OUT_NAME" "$FINAL" 2>/dev/null || cp -a "$STAGE/$OUT_NAME" "$FINAL" || die "Could not write $FINAL"

printf '\n%sBuild succeeded.%s\n' "$G" "$N"
info "Addon:         $FINAL/addons/vimdow"
info "Platform:      $PLATFORM_DESC ($TARGET_TRIPLE), profiles: ${PROFILES[*]}"
info "Install:       cp -r \"$FINAL/addons/vimdow\" <your-godot-project>/addons/"
info "               then enable 'Vimdow' in Project > Project Settings > Plugins"
