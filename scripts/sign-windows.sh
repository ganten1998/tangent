#!/usr/bin/env bash
# Authenticode-sign ONE Windows PE via Azure Artifact Signing (formerly Trusted
# Signing) using jsign, which unlike Microsoft's signtool runs on macOS/Linux.
# No certificate ever touches this machine: jsign sends the file hash to Azure,
# Microsoft's HSM signs it and returns the signature.
#
#   scripts/sign-windows.sh path/to/some.exe
#
# One script and two callers, deliberately. build-cross.sh signs the inner
# tangent.exe before it is staged, and signs the NSIS setup.exe after makensis
# writes it — SmartScreen judges the OUTER installer, so signing the inner exe
# alone ships the warning anyway. jsign signs any PE, NSIS output included
# (Authenticode is appended to the PE; the NSIS CRC lives inside it and is not
# disturbed — signing installers post-build is the standard flow). Keeping the
# route in one file means the two signatures can never take different paths.
#
# Unconfigured => the PE ships UNSIGNED (the documented pre-Azure state) and
# this says so once. PARTIALLY configured => hard failure, because silently
# shipping an unsigned binary while believing it is signed is the worst outcome
# here. Either way release.sh has the last word: it refuses an unsigned
# setup.exe at artifact-check time (scripts/pe-signed.py), exactly as it
# refuses an unsigned .pkg.
#
# Residual gap, documented rather than hidden: NSIS generates Uninstall.exe on
# the user's machine at install time, so it cannot be signed from here without
# the NSIS uninstaller-stub signing dance. Out of scope.
#
# Setup (see docs/RELEASE.md § Windows):
#   brew install jsign            # needs a JRE
#   brew install azure-cli        # only for the default token path
#   export TRUSTED_SIGNING_ENDPOINT=https://eus.codesigning.azure.net
#   export TRUSTED_SIGNING_ACCOUNT=<your-account>
#   export TRUSTED_SIGNING_PROFILE=<your-certificate-profile>
#   # token: taken from `az account get-access-token` unless you set
#   # TRUSTED_SIGNING_TOKEN yourself (e.g. from a CI federated credential).
set -euo pipefail

EXE="${1:-}"
[ -n "$EXE" ] || { echo "usage: $0 PE-FILE" >&2; exit 2; }
[ -f "$EXE" ] || { echo "$0: no such file: $EXE" >&2; exit 2; }

set_count=0
for v in "${TRUSTED_SIGNING_ENDPOINT:-}" "${TRUSTED_SIGNING_ACCOUNT:-}" \
         "${TRUSTED_SIGNING_PROFILE:-}"; do
  [ -n "$v" ] && set_count=$((set_count + 1))
done

if [ "$set_count" = 0 ]; then
  echo "    (unsigned: TRUSTED_SIGNING_* not set — Windows will show SmartScreen)"
  exit 0
fi
if [ "$set_count" != 3 ]; then
  echo "TRUSTED_SIGNING_* is only partially set — refusing to ship a silently" >&2
  echo "unsigned PE. Set ENDPOINT, ACCOUNT and PROFILE, or none of them." >&2
  exit 1
fi

command -v jsign >/dev/null 2>&1 || {
  echo "jsign not found (brew install jsign) but TRUSTED_SIGNING_* is set." >&2
  exit 1
}

token="${TRUSTED_SIGNING_TOKEN:-}"
if [ -z "$token" ]; then
  command -v az >/dev/null 2>&1 || {
    echo "Need an access token: install azure-cli, or set TRUSTED_SIGNING_TOKEN." >&2
    exit 1
  }
  token="$(az account get-access-token \
    --resource https://codesigning.azure.net \
    --query accessToken -o tsv)" || {
    echo "az could not get a token — run 'az login' first." >&2; exit 1; }
fi

echo "==> Signing $(basename "$EXE") via Azure Artifact Signing"
# RFC-3161 timestamp so the signature outlives the short-lived certificate.
jsign --storetype TRUSTEDSIGNING \
      --keystore "$TRUSTED_SIGNING_ENDPOINT" \
      --storepass "$token" \
      --alias "${TRUSTED_SIGNING_ACCOUNT}/${TRUSTED_SIGNING_PROFILE}" \
      --tsaurl http://timestamp.acs.microsoft.com \
      --name "Tangent" \
      "$EXE" || { echo "jsign failed — $(basename "$EXE") NOT signed" >&2; exit 1; }

# Best-effort local check; real proof is a first launch on Windows.
if command -v osslsigncode >/dev/null 2>&1; then
  osslsigncode verify "$EXE" 2>&1 | grep -iE "signature|signer|timestamp" \
    | head -4 | sed 's/^/    /' || true
else
  echo "    (install osslsigncode to verify the signature here)"
fi
