#!/bin/sh
# Installs a Folio package the way a user would and checks it is whole. Runs inside a plain
# distro image (see the package-smoke job in .github/workflows/ci.yml), so it finds what the
# package forgot to depend on: only its hard dependencies are installed, not the extras it
# recommends. It proves the program installs, links and stays running; it does not prove the
# web view drew anything (see docs/LINUX.md for the manual rendering QA).
# Usage: linux-smoke.sh <path to .deb or .rpm>
set -eu

pkg="$1"
case "$pkg" in
  *.deb)
    export DEBIAN_FRONTEND=noninteractive
    apt-get update -qq
    # The screen to run on comes first, on its own, so the flags below apply to Folio only.
    apt-get install -y -qq xvfb
    # apt resolves the package's own Depends from the distro: that is the point of the test.
    # Recommends are left out, or one could hide a missing Depends.
    apt-get install -y -qq --no-install-recommends "$pkg"
    ;;
  *.rpm)
    dnf install -y -q xorg-x11-server-Xvfb xorg-x11-xauth
    dnf install -y -q --setopt=install_weak_deps=False "$pkg"
    ;;
  *)
    echo "not a package: $pkg" >&2
    exit 2
    ;;
esac

bin=$(command -v folio || true)
[ -n "$bin" ] || { echo "no folio on PATH after install" >&2; exit 1; }

# Every library the program links must be there.
missing=$(ldd "$bin" | grep 'not found' || true)
if [ -n "$missing" ]; then
  echo "missing libraries:" >&2
  echo "$missing" >&2
  exit 1
fi

# The launcher entry and its icon are installed.
desktop=$(ls /usr/share/applications/*olio*.desktop 2>/dev/null | head -n 1 || true)
[ -n "$desktop" ] || { echo "no .desktop file installed" >&2; exit 1; }
grep -q '^Exec=' "$desktop" || { echo "$desktop has no Exec line" >&2; exit 1; }
ls /usr/share/icons/hicolor/*/apps/*olio*.png >/dev/null 2>&1 || {
  echo "no icon installed" >&2
  exit 1
}
# The bundled licenses and plugins sit next to the program's resources.
find /usr -name THIRD-PARTY-LICENSES.txt | grep -q . || {
  echo "THIRD-PARTY-LICENSES.txt not installed" >&2
  exit 1
}
find /usr -type d -name plugins -path '*olio*' | grep -q . || {
  echo "the bundled plugins folder is not installed" >&2
  exit 1
}

# It starts and stays up for a few seconds on a virtual screen. Exit 124 is the timeout
# stopping a program that was still running, which is what a healthy start looks like.
# WebKit's sandbox can't make namespaces as root in a container, so it is switched off for
# this test only; without that the web process could die while the program stays up.
set +e
WEBKIT_DISABLE_COMPOSITING_MODE=1 WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS=1 \
  timeout 15 xvfb-run -a "$bin" 2>/tmp/folio.err
code=$?
set -e
if [ "$code" -ne 124 ]; then
  echo "folio exited early with code $code; its messages:" >&2
  cat /tmp/folio.err >&2
  exit 1
fi
echo "ok: $pkg"
