#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 1 ]]; then
  echo "usage: $0 <client-name>" >&2
  exit 1
fi

CLIENT="$1"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
OUT_DIR="${SCRIPT_DIR}/out"
CA_KEY="${OUT_DIR}/ca/lokan-dev-root-ca.key.pem"
CA_CERT="${OUT_DIR}/ca/lokan-dev-root-ca.cert.pem"

if [[ ! -f "${CA_KEY}" || ! -f "${CA_CERT}" ]]; then
  echo "Development CA not found. Run generate_ca.sh first." >&2
  exit 1
fi

CLIENT_DIR="${OUT_DIR}/clients/${CLIENT}"
mkdir -p "${CLIENT_DIR}"

KEY_PATH="${CLIENT_DIR}/${CLIENT}.key.pem"
CSR_PATH="${CLIENT_DIR}/${CLIENT}.csr.pem"
CERT_PATH="${CLIENT_DIR}/${CLIENT}.cert.pem"

openssl genrsa -out "${KEY_PATH}" 2048
openssl req -new -key "${KEY_PATH}" -out "${CSR_PATH}" -subj "/CN=${CLIENT}" \
  -addext "extendedKeyUsage=clientAuth"

openssl x509 -req -in "${CSR_PATH}" -CA "${CA_CERT}" -CAkey "${CA_KEY}" \
  -CAcreateserial -out "${CERT_PATH}" -days 90 -sha256 \
  -extfile <(printf "extendedKeyUsage = clientAuth")

cat <<INFO
Issued development client certificate for ${CLIENT}.
  Private Key : ${KEY_PATH}
  Certificate : ${CERT_PATH}
  CSR         : ${CSR_PATH}
INFO
