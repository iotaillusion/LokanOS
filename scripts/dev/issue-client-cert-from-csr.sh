#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 2 ]]; then
  echo "usage: $0 <device-id> <csr-pem-path>" >&2
  exit 1
fi

DEVICE_ID="$1"
CSR_PATH="$2"

if [[ ! -f "$CSR_PATH" ]]; then
  echo "csr file '$CSR_PATH' does not exist" >&2
  exit 1
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PKI_DIR="${SCRIPT_DIR}/../../security/pki/dev"
OUT_DIR="${PKI_DIR}/out/commissioned/${DEVICE_ID}"
CA_KEY="${PKI_DIR}/out/ca/lokan-dev-root-ca.key.pem"
CA_CERT="${PKI_DIR}/out/ca/lokan-dev-root-ca.cert.pem"

if [[ ! -f "${CA_KEY}" || ! -f "${CA_CERT}" ]]; then
  echo "Development CA not found. Run security/pki/dev/generate_ca.sh first." >&2
  exit 1
fi

mkdir -p "${OUT_DIR}"
CERT_PATH="${OUT_DIR}/${DEVICE_ID}.cert.pem"
BASE64_PATH="${OUT_DIR}/${DEVICE_ID}.cert.base64"

openssl x509 -req -in "${CSR_PATH}" -CA "${CA_CERT}" -CAkey "${CA_KEY}" \
  -CAcreateserial -out "${CERT_PATH}" -days 90 -sha256 \
  -extfile <(printf "extendedKeyUsage = clientAuth")

base64 -w0 "${CERT_PATH}" > "${BASE64_PATH}"

cat <<INFO
Issued commissioned client certificate for ${DEVICE_ID}.
  CSR          : ${CSR_PATH}
  Certificate  : ${CERT_PATH}
  Certificate (base64): ${BASE64_PATH}
INFO
