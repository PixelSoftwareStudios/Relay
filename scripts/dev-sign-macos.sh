#!/bin/bash
# Gives local Relay dev binaries a STABLE code identity on macOS so that choosing
# "Always Allow" in the keychain prompt for the "relay-voice" item survives rebuilds.
#
# Why: the keychain remembers an app by its designated requirement. An unsigned or
# ad-hoc signed Rust binary has a requirement built from its cdhash, which changes on
# every build. Signed with a fixed identifier and a fixed (self-signed) certificate, the
# requirement becomes `identifier "dev.relay.relayd" and certificate leaf = H"<hash>"`,
# which does not change when the code changes.
#
# What it does (nothing needs sudo, nothing touches keychain trust settings):
#   1. Once: creates a self-signed code-signing certificate "Relay Local Dev Signing"
#      with openssl and imports certificate + key into your login keychain.
#      macOS may ask for your login password for the import. The certificate is NOT
#      marked trusted (no `add-trusted-cert`); codesign does not need trust.
#   2. Signs relayd, relay and relay-desktop in target/<profile>/ with ONE identifier,
#      dev.relay.relayd: every client starts the daemon from its own executable, so all
#      three must satisfy the same keychain requirement. Already correctly signed
#      binaries are left alone, so re-running is cheap and idempotent.
#
# Usage:
#   scripts/dev-sign-macos.sh [--release] [--setup-only] [--no-cert] [PATH...]
#     --release     sign target/release instead of target/debug
#     --setup-only  only create the certificate
#     --no-cert     do not create or use a certificate: ad-hoc signature with an
#                   explicit requirement `designated => identifier "<id>"`. Weaker
#                   (any ad-hoc binary with that identifier satisfies it) but needs
#                   no keychain change at all.
#     PATH...       sign these files instead (same identifier)
#
# Environment (for tests): RELAY_DEV_KEYCHAIN (default login keychain),
# RELAY_DEV_KEYCHAIN_PASSWORD (if set, lets codesign use the key without a prompt),
# RELAY_DEV_SIGN_NO_CREATE (set by the cargo runner: never create the certificate).
#
# This is development ergonomics only. Developer ID signing, hardened runtime and
# entitlements for release builds are a separate, still open gate (docs/gates.md).
set -euo pipefail

CERT_NAME="Relay Local Dev Signing"
ID_PREFIX="dev.relay"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PROFILE=debug
SETUP_ONLY=0
NO_CERT=0
FILES=()

for arg in "$@"; do
	case "$arg" in
	--release) PROFILE=release ;;
	--setup-only) SETUP_ONLY=1 ;;
	--no-cert) NO_CERT=1 ;;
	-h | --help) sed -n '2,38p' "$0"; exit 0 ;;
	-*) echo "unknown option $arg" >&2; exit 2 ;;
	*) FILES+=("$arg") ;;
	esac
done

if [ "$(uname -s)" != Darwin ]; then
	echo "dev-sign-macos: not macOS, nothing to do" >&2
	exit 0
fi

KEYCHAIN="${RELAY_DEV_KEYCHAIN:-$(security default-keychain -d user | sed -e 's/^ *"//' -e 's/"$//')}"
OPENSSL=/usr/bin/openssl
[ -x "$OPENSSL" ] || OPENSSL=openssl

# SHA-1 of the first matching certificate, empty when missing.
cert_hash() {
	security find-certificate -c "$CERT_NAME" -Z "$KEYCHAIN" 2>/dev/null |
		awk '/^SHA-1 hash:/ { print $3; exit }'
}

# Is there a usable identity (certificate with private key)?
have_identity() {
	security find-identity -p codesigning "$KEYCHAIN" 2>/dev/null | grep -q "\"$CERT_NAME\""
}

create_cert() {
	local tmp
	tmp="$(mktemp -d)"
	trap 'rm -rf "$tmp"' RETURN
	echo "Creating self-signed code-signing certificate \"$CERT_NAME\" (valid 10 years)"
	echo "and importing it into: $KEYCHAIN"
	cat >"$tmp/openssl.cnf" <<CNF
[req]
distinguished_name = dn
x509_extensions = ext
prompt = no
[dn]
CN = $CERT_NAME
[ext]
basicConstraints = critical,CA:false
keyUsage = critical,digitalSignature
extendedKeyUsage = critical,codeSigning
CNF
	"$OPENSSL" req -x509 -newkey rsa:2048 -nodes -days 3650 \
		-keyout "$tmp/key.pem" -out "$tmp/cert.pem" -config "$tmp/openssl.cnf" 2>/dev/null
	local pass="relay-dev-$$"
	# Old-style PKCS#12 encryption: `security import` rejects newer defaults.
	"$OPENSSL" pkcs12 -export -inkey "$tmp/key.pem" -in "$tmp/cert.pem" \
		-out "$tmp/id.p12" -passout "pass:$pass" \
		-keypbe PBE-SHA1-3DES -certpbe PBE-SHA1-3DES -macalg sha1 2>/dev/null
	security import "$tmp/id.p12" -k "$KEYCHAIN" -P "$pass" -T /usr/bin/codesign >/dev/null
	if [ -n "${RELAY_DEV_KEYCHAIN_PASSWORD:-}" ]; then
		security set-key-partition-list -S apple-tool:,apple: -s \
			-k "$RELAY_DEV_KEYCHAIN_PASSWORD" "$KEYCHAIN" >/dev/null
	fi
	echo "Done. The first signing may show \"codesign wants to sign using key\":"
	echo "choose Always Allow. The certificate was not marked trusted, by design."
}

sign_one() {
	local file="$1" ident="$2" hash="$3"
	[ -f "$file" ] || { echo "skip (not built): $file"; return 0; }
	local want current
	if [ -n "$hash" ]; then
		want="identifier \"$ident\" and certificate leaf = H\"$(echo "$hash" | tr 'A-F' 'a-f')\""
	else
		want="identifier \"$ident\""
	fi
	current="$(codesign -d -r- "$file" 2>&1 | sed -n 's/^designated => //p')"
	if [ "$current" = "$want" ] && codesign --verify "$file" 2>/dev/null; then
		echo "ok (already signed): $file"
		return 0
	fi
	if [ -n "$hash" ]; then
		codesign --force --keychain "$KEYCHAIN" --sign "$hash" --identifier "$ident" --timestamp=none "$file"
	else
		codesign --force --sign - --identifier "$ident" --timestamp=none \
			-r="designated => identifier \"$ident\"" "$file"
	fi
	echo "signed: $file as $ident"
}

HASH=""
if [ "$NO_CERT" = 0 ]; then
	if ! have_identity; then
		# The cargo runner sets this: never create a certificate implicitly.
		[ -z "${RELAY_DEV_SIGN_NO_CREATE:-}" ] || exit 0
		create_cert
	fi
	HASH="$(cert_hash)"
	[ -n "$HASH" ] || { echo "certificate not found after import" >&2; exit 1; }
fi
[ "$SETUP_ONLY" = 1 ] && exit 0

if [ ${#FILES[@]} -eq 0 ]; then
	for bin in relayd relay relay-desktop; do
		FILES+=("$ROOT/target/$PROFILE/$bin")
	done
fi
for f in "${FILES[@]}"; do
	sign_one "$f" "$ID_PREFIX.relayd" "$HASH"
done
