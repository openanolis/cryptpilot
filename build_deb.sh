#!/bin/bash
set -euo pipefail

PACKAGE_NAME="cryptpilot"
VERSION="${1:-$(grep '^version' Cargo.toml | awk -F' = ' '{print $2}' | tr -d '\"')}"
RELEASE_NUM="${2:-1}"
ARCH="amd64"
BUILD_DIR="$(pwd)/build"
DIST_DIR="$(pwd)/dist"

TARBALL="/tmp/${PACKAGE_NAME}-${VERSION}.tar.gz"
USE_TARBALL=1

# prepare workspace
rm -rf "$BUILD_DIR"
mkdir -p "$BUILD_DIR"

if [ -f "$TARBALL" ]; then
    echo "=== Using tarball: $TARBALL ==="
    EXTRACT_DIR="$BUILD_DIR/${PACKAGE_NAME}-${VERSION}"
    mkdir -p "$EXTRACT_DIR"
    echo "=== Extracting $TARBALL to $EXTRACT_DIR ==="
    tar -xzf "$TARBALL" -C "$BUILD_DIR"
    SRC_PATH="$EXTRACT_DIR/src"
    INSTALL_ROOT="$EXTRACT_DIR/install"
else
    echo "=== Tarball not found, building from local source ==="
    USE_TARBALL=0
    SRC_PATH="$(pwd)"
    INSTALL_ROOT="$BUILD_DIR/install"
fi

if [ ! -f "$SRC_PATH/Cargo.toml" ]; then
    echo "ERROR: Cargo.toml not found in source path: $SRC_PATH"
    exit 3
fi

# Build/install binary from extracted source
echo "=== Building cryptpilot binary from $SRC_PATH ==="
# Use an array for flags to avoid word splitting (SC2086)
CARGO_FLAGS=("--locked")
if [ "$USE_TARBALL" -eq 1 ]; then
    CARGO_FLAGS+=("--offline")
fi
cargo install --path "$SRC_PATH" --bin cryptpilot --root "$INSTALL_ROOT" "${CARGO_FLAGS[@]}"

# Prepare package filesystem tree
PKG_DIR="$BUILD_DIR/pkg"
rm -rf "$PKG_DIR"
mkdir -p "$PKG_DIR"

echo "=== Installing files into package tree ==="

# Binaries
mkdir -p "$PKG_DIR/usr/bin"
cp -p "$INSTALL_ROOT/bin/cryptpilot" "$PKG_DIR/usr/bin/cryptpilot"
cp -p ./cryptpilot-convert.sh "$PKG_DIR/usr/bin/cryptpilot-convert" || true
chmod 755 "$PKG_DIR/usr/bin"/* || true

# Dracut modules (from repo dist)
mkdir -p "$PKG_DIR/usr/lib/dracut/modules.d/91cryptpilot"
cp -p "$DIST_DIR/dracut/modules.d/91cryptpilot/"* "$PKG_DIR/usr/lib/dracut/modules.d/91cryptpilot/" 2>/dev/null || true
# Avoid A && B || C pattern (SC2015); run chmod only if dir exists
if [ -d "$PKG_DIR/usr/lib/dracut/modules.d/91cryptpilot" ]; then
    chmod 755 "$PKG_DIR/usr/lib/dracut/modules.d/91cryptpilot"/*.sh 2>/dev/null || true
fi

# Systemd service
mkdir -p "$PKG_DIR/lib/systemd/system"
cp -p "$DIST_DIR/systemd/cryptpilot.service" "$PKG_DIR/lib/systemd/system/" 2>/dev/null || true

# Config templates
mkdir -p "$PKG_DIR/etc/cryptpilot/volumes"
cp -p "$DIST_DIR/etc/global.toml.template" "$PKG_DIR/etc/cryptpilot/global.toml.template" 2>/dev/null || true
cp -p "$DIST_DIR/etc/fde.toml.template" "$PKG_DIR/etc/cryptpilot/fde.toml.template" 2>/dev/null || true
cp -p "$DIST_DIR/etc/volumes/"* "$PKG_DIR/etc/cryptpilot/volumes/" 2>/dev/null || true

# udev rules
mkdir -p "$PKG_DIR/usr/lib/udev/rules.d"
cp -p "$DIST_DIR/usr/lib/udev/rules.d/12-cryptpilot-hide-intermediate-devices.rules" "$PKG_DIR/usr/lib/udev/rules.d/" 2>/dev/null || true

# policy files
mkdir -p "$PKG_DIR/usr/share/cryptpilot"
cp -p "$DIST_DIR/usr/share/cryptpilot/policy.rego" "$PKG_DIR/usr/share/cryptpilot/" 2>/dev/null || true

# DEBIAN control
mkdir -p "$PKG_DIR/DEBIAN"
cat > "$PKG_DIR/DEBIAN/control" <<CONTROL_EOF
Package: $PACKAGE_NAME
Version: $VERSION-$RELEASE_NUM
Section: utils
Priority: optional
Architecture: $ARCH
Maintainer: Kun Lai <laikun@linux.alibaba.com>
Depends: dracut, dracut-network,lvm2, cryptsetup, coreutils, systemd, kmod, dosfstools, xfsprogs, e2fsprogs, util-linux, qemu-utils, file
Recommends: confidential-data-hub
Suggests: attestation-agent
Description: A utility for protecting data at rest in confidential environment
CONTROL_EOF

# postinst
cat > "$PKG_DIR/DEBIAN/postinst" <<'POSTINST_EOF'
#!/bin/sh
set -e
if command -v systemctl >/dev/null 2>&1; then
    systemctl daemon-reload || true
fi
if command -v udevadm >/dev/null 2>&1; then
    udevadm control --reload-rules || true
fi
exit 0
POSTINST_EOF
chmod 755 "$PKG_DIR/DEBIAN/postinst"

# prerm
cat > "$PKG_DIR/DEBIAN/prerm" <<'PRERM_EOF'
#!/bin/sh
set -e
if [ "$1" = "remove" ] || [ "$1" = "upgrade" ]; then
    if command -v systemctl >/dev/null 2>&1; then
        systemctl stop cryptpilot.service || true
        systemctl disable cryptpilot.service || true
    fi
fi
exit 0
PRERM_EOF
chmod 755 "$PKG_DIR/DEBIAN/prerm"

# postrm
cat > "$PKG_DIR/DEBIAN/postrm" <<'POSTRM_EOF'
#!/bin/sh
set -e
if command -v systemctl >/dev/null 2>&1; then
    systemctl daemon-reload || true
    systemctl reset-failed || true
fi
exit 0
POSTRM_EOF
chmod 755 "$PKG_DIR/DEBIAN/postrm"

# Build .deb
OUTPUT_DEB="$BUILD_DIR/${PACKAGE_NAME}_${VERSION}-${RELEASE_NUM}_${ARCH}.deb"
echo "=== Building DEB: $OUTPUT_DEB ==="
dpkg-deb --build "$PKG_DIR" "$OUTPUT_DEB"

echo "Done: $OUTPUT_DEB"
