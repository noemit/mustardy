#!/usr/bin/env sh
set -eu

REPO="noemit/mustardy"
INSTALL_DIR="${MUSTARDY_INSTALL_DIR:-$HOME/.local/bin}"

OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
  Darwin) OS_PART="apple-darwin" ;;
  Linux)  OS_PART="unknown-linux-gnu" ;;
  *)
    echo "mustardy: unsupported OS '$OS' (supported: macOS, Linux)" >&2
    exit 1
    ;;
esac

case "$ARCH" in
  arm64|aarch64) ARCH_PART="aarch64" ;;
  x86_64|amd64)  ARCH_PART="x86_64" ;;
  *)
    echo "mustardy: unsupported architecture '$ARCH' (supported: arm64/aarch64, x86_64)" >&2
    exit 1
    ;;
esac

TARGET="${ARCH_PART}-${OS_PART}"

if [ -n "${MUSTARDY_VERSION:-}" ]; then
  TAG="$MUSTARDY_VERSION"
else
  TAG="$(curl -fsSLI -o /dev/null -w '%{url_effective}' "https://github.com/${REPO}/releases/latest" | sed 's#.*/##')"
fi

if [ -z "$TAG" ] || [ "$TAG" = "releases" ]; then
  echo "mustardy: could not determine latest release tag" >&2
  exit 1
fi

ARCHIVE="mustardy-${TAG}-${TARGET}.tar.gz"
BASE_URL="https://github.com/${REPO}/releases/download/${TAG}"

TMPDIR_WORK="$(mktemp -d)"
trap 'rm -rf "$TMPDIR_WORK"' EXIT

echo "mustardy: downloading ${ARCHIVE}..."
curl -fsSL "${BASE_URL}/${ARCHIVE}" -o "${TMPDIR_WORK}/${ARCHIVE}"
curl -fsSL "${BASE_URL}/SHA256SUMS.txt" -o "${TMPDIR_WORK}/SHA256SUMS.txt"

EXPECTED="$(grep " ${ARCHIVE}\$" "${TMPDIR_WORK}/SHA256SUMS.txt" | awk '{print $1}')"
if [ -n "$EXPECTED" ]; then
  if command -v sha256sum >/dev/null 2>&1; then
    ACTUAL="$(sha256sum "${TMPDIR_WORK}/${ARCHIVE}" | awk '{print $1}')"
  elif command -v shasum >/dev/null 2>&1; then
    ACTUAL="$(shasum -a 256 "${TMPDIR_WORK}/${ARCHIVE}" | awk '{print $1}')"
  else
    ACTUAL=""
  fi
  if [ -n "$ACTUAL" ] && [ "$ACTUAL" != "$EXPECTED" ]; then
    echo "mustardy: checksum mismatch (expected ${EXPECTED}, got ${ACTUAL})" >&2
    exit 1
  fi
fi

tar -xzf "${TMPDIR_WORK}/${ARCHIVE}" -C "${TMPDIR_WORK}"
mkdir -p "$INSTALL_DIR"
BIN_PATH="$(find "${TMPDIR_WORK}" -type f -name mustardy | head -n 1)"
if [ -z "$BIN_PATH" ]; then
  echo "mustardy: binary not found inside ${ARCHIVE}" >&2
  exit 1
fi

install -m 755 "$BIN_PATH" "${INSTALL_DIR}/mustardy"
echo "mustardy: installed ${TAG} to ${INSTALL_DIR}/mustardy"

case ":${PATH}:" in
  *":${INSTALL_DIR}:"*) ;;
  *)
    echo "mustardy: add ${INSTALL_DIR} to your PATH, e.g.:"
    echo "  export PATH=\"${INSTALL_DIR}:\$PATH\""
    ;;
esac

if ! command -v ffmpeg >/dev/null 2>&1; then
  echo "mustardy: note — ffmpeg is not on PATH yet (install via 'brew install ffmpeg' or your package manager)"
fi

echo "mustardy: run 'mustardy doctor' to verify your setup"
