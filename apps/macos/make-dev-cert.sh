#!/usr/bin/env bash
# Creates a self-signed "Spent Day Dev" code-signing identity in the login keychain, so
# local builds keep a stable signature. macOS may ask for your login password to trust it.
# It trusts the certificate for code signing and lets /usr/bin/codesign use the key without
# asking, so any program running as you can sign code as "Spent Day Dev". The README
# explains the trade-off and the alternative, an Apple Development certificate.
set -euo pipefail

name="Spent Day Dev"
keychain="$HOME/Library/Keychains/login.keychain-db"

if security find-identity -v -p codesigning "$keychain" | grep -q "\"$name\""; then
  echo "\"$name\" already exists."
  exit 0
fi
# One that isn't valid, because it is untrusted, expired or half made, would clash by name.
if security find-identity -p codesigning "$keychain" | grep -q "\"$name\""; then
  echo "A \"$name\" that isn't valid is in your keychain. It may be untrusted or expired." >&2
  echo "Remove it with: security delete-identity -c \"$name\" -t" >&2
  echo "Then run this script again." >&2
  exit 1
fi

work="$(mktemp -d)"
hash=""
made=0
# A failure or an interrupt after an import removes what was imported, so a rerun starts
# clean. It checks a flag, not $?, because $? can be 0 in an EXIT trap after a signal.
finish() {
  if [ "$made" != 1 ] && [ -n "$hash" ]; then
    security delete-identity -Z "$hash" -t "$keychain" >/dev/null 2>&1 \
      || security delete-certificate -Z "$hash" -t "$keychain" >/dev/null 2>&1 || true
    echo "Removed the partly made \"$name\"." >&2
  fi
  rm -rf "$work"
}
trap finish EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM

cat > "$work/cert.cnf" <<CNF
[req]
distinguished_name = dn
x509_extensions = ext
prompt = no
[dn]
CN = $name
[ext]
basicConstraints = critical, CA:false
keyUsage = critical, digitalSignature
extendedKeyUsage = critical, codeSigning
CNF

# The certificate and the key are imported one by one, so no PKCS#12 file and no password
# are needed. The key stays in a temp dir only you can read, which is removed on exit.
# In the keychain the key can't be exported (-x).
/usr/bin/openssl req -x509 -newkey rsa:2048 -nodes -days 3650 \
  -config "$work/cert.cnf" -keyout "$work/key.pem" -out "$work/cert.pem"
fingerprint="$(/usr/bin/openssl x509 -in "$work/cert.pem" -noout -fingerprint -sha1)"
fingerprint="${fingerprint#*=}"
hash="${fingerprint//:/}"

security import "$work/cert.pem" -k "$keychain"
security import "$work/key.pem" -k "$keychain" -x -T /usr/bin/codesign
security add-trusted-cert -r trustRoot -p codeSign -k "$keychain" "$work/cert.pem"

security find-identity -v -p codesigning "$keychain" | grep "\"$name\""
made=1
echo "Created \"$name\". Rebuild with apps/macos/build.sh to use it."
echo "To remove it: security delete-identity -c \"$name\" -t"
