#!/usr/bin/env bash
# Publish a freshly-built unsigned MeridianRunner IPA to the Meridian VPS and
# regenerate the manifest the hub reads when sideloading.
#
# Usage:
#   publish_ipa.sh <version> <path-to-MeridianRunner-unsigned.ipa>
#
# Example:
#   publish_ipa.sh 0.1.0 ../../prebuilt/MeridianRunner-unsigned.ipa
#
# The IPA is usually the GitHub Actions artifact produced by
# .github/workflows/unified-runner.yml.
set -euo pipefail

VERSION="${1:?usage: publish_ipa.sh <version> <ipa-file>}"
IPA_FILE="${2:?usage: publish_ipa.sh <version> <ipa-file>}"
[ -f "$IPA_FILE" ] || { echo "ERROR: IPA not found: $IPA_FILE"; exit 1; }

VPS_HOST="98.84.189.148"
VPS_USER="admin"
VPS_DIR="/var/www/meridian-runner"
BASE_URL="https://meridianhub.cc/runner"

SHA256="$(sha256sum "$IPA_FILE" | awk '{print $1}')"
SIZE="$(stat -c%s "$IPA_FILE")"
DEST_NAME="MeridianRunner-unsigned.ipa"

echo "Publishing v${VERSION} (${SIZE} bytes, sha256 ${SHA256:0:16}...)"
scp "$IPA_FILE" "${VPS_USER}@${VPS_HOST}:${VPS_DIR}/${DEST_NAME}"

MANIFEST="manifest.json"
cat > "$MANIFEST" <<EOF
{
  "version": "${VERSION}",
  "url": "${BASE_URL}/${DEST_NAME}",
  "sha256": "${SHA256}",
  "size": ${SIZE}
}
EOF

scp "$MANIFEST" "${VPS_USER}@${VPS_HOST}:${VPS_DIR}/${MANIFEST}"
rm -f "$MANIFEST"
echo "✓ Published: ${BASE_URL}/${DEST_NAME}  (v${VERSION})"