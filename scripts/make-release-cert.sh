#!/usr/bin/env bash
set -euo pipefail
umask 077

work="$(mktemp -d "${TMPDIR:-/tmp}/spentday-release-cert.XXXXXX")"
printf 'Writing release signing files to %s\n' "$work"
cat > "$work/cert.cnf" <<'CNF'
[req]
distinguished_name = dn
x509_extensions = ext
prompt = no
[dn]
CN = Spent Day Release
[ext]
basicConstraints = critical, CA:false
keyUsage = critical, digitalSignature
extendedKeyUsage = critical, codeSigning
CNF

openssl req -x509 -newkey rsa:3072 -nodes -days 3650 \
  -config "$work/cert.cnf" -keyout "$work/key.pem" -out "$work/cert.pem"
openssl rand -hex 32 | tr -d '\n' > "$work/password.txt"
# macOS security import cannot read OpenSSL's default PBES2 PKCS#12 encryption.
openssl pkcs12 -export -name 'Spent Day Release' \
  -inkey "$work/key.pem" -in "$work/cert.pem" -out "$work/signing.p12" \
  -passout "file:$work/password.txt" \
  -keypbe PBE-SHA1-3DES -certpbe PBE-SHA1-3DES -macalg sha1
openssl base64 -A -in "$work/signing.p12" -out "$work/signing.p12.base64"

printf '\nRun these commands once from an authenticated GitHub CLI:\n'
printf 'gh secret set MACOS_SIGN_P12 --repo cravenceiling/spent-day < %q\n' "$work/signing.p12.base64"
printf 'gh secret set MACOS_SIGN_P12_PASSWORD --repo cravenceiling/spent-day < %q\n' "$work/password.txt"
printf '\nKeep signing.p12 and password.txt together in a private backup. Reuse this identity for updates.\n'
printf 'Remove the temporary directory after saving the files and setting the secrets.\n'
