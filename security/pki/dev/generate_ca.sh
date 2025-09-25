#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
OUT_DIR="${SCRIPT_DIR}/out"
CA_DIR="${OUT_DIR}/ca"
mkdir -p "${CA_DIR}"

CA_KEY="${CA_DIR}/lokan-dev-root-ca.key.pem"
CA_CERT="${CA_DIR}/lokan-dev-root-ca.cert.pem"

if [[ -f "${CA_KEY}" || -f "${CA_CERT}" ]]; then
  echo "[!] Existing CA artifacts found in ${CA_DIR}." >&2
  echo "    Remove them first if you want to reissue the development CA." >&2
  exit 1
fi

openssl genrsa -out "${CA_KEY}" 4096
openssl req -x509 -new -nodes -key "${CA_KEY}" -sha256 -days 365 \
  -subj "/C=US/O=LokanOS Dev/CN=Lokan Dev Root CA" \
  -out "${CA_CERT}"

cat <<INFO
Generated development root CA.
  Private Key : ${CA_KEY}
  Certificate : ${CA_CERT}
INFO
