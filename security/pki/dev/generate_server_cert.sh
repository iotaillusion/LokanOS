#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 1 ]]; then
  echo "usage: $0 <service-name> [dns-names]" >&2
  echo "  dns-names: optional comma separated list of subjectAltName DNS entries" >&2
  exit 1
fi

SERVICE="$1"
ALT_NAMES="${2:-}" # comma separated

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
OUT_DIR="${SCRIPT_DIR}/out"
CA_KEY="${OUT_DIR}/ca/lokan-dev-root-ca.key.pem"
CA_CERT="${OUT_DIR}/ca/lokan-dev-root-ca.cert.pem"

if [[ ! -f "${CA_KEY}" || ! -f "${CA_CERT}" ]]; then
  echo "Development CA not found. Run generate_ca.sh first." >&2
  exit 1
fi

CERT_DIR="${OUT_DIR}/services/${SERVICE}"
mkdir -p "${CERT_DIR}"

KEY_PATH="${CERT_DIR}/${SERVICE}.key.pem"
CSR_PATH="${CERT_DIR}/${SERVICE}.csr.pem"
CERT_PATH="${CERT_DIR}/${SERVICE}.cert.pem"

openssl genrsa -out "${KEY_PATH}" 2048
openssl req -new -key "${KEY_PATH}" -out "${CSR_PATH}" -subj "/CN=${SERVICE}" \
  -addext "extendedKeyUsage=serverAuth"

SAN_CONFIG=""
if [[ -n "${ALT_NAMES}" ]]; then
  TMP_CFG="$(mktemp)"
  {
    echo "subjectAltName = @alt_names"
    echo "[alt_names]"
    IFS=',' read -ra ENTRIES <<<"${ALT_NAMES}"
    IDX=1
    for entry in "${ENTRIES[@]}"; do
      entry="$(echo "${entry}" | xargs)"
      [[ -z "${entry}" ]] && continue
      echo "DNS.${IDX} = ${entry}"
      IDX=$((IDX+1))
    done
  } > "${TMP_CFG}"
  SAN_CONFIG="-extfile ${TMP_CFG}"
fi

set +e
if [[ -n "${SAN_CONFIG}" ]]; then
  openssl x509 -req -in "${CSR_PATH}" -CA "${CA_CERT}" -CAkey "${CA_KEY}" \
    -CAcreateserial -out "${CERT_PATH}" -days 90 -sha256 ${SAN_CONFIG}
  STATUS=$?
  rm -f "${TMP_CFG}"
else
  openssl x509 -req -in "${CSR_PATH}" -CA "${CA_CERT}" -CAkey "${CA_KEY}" \
    -CAcreateserial -out "${CERT_PATH}" -days 90 -sha256 \
    -extfile <(printf "extendedKeyUsage = serverAuth")
  STATUS=$?
fi
set -e

if [[ ${STATUS} -ne 0 ]]; then
  echo "Failed to sign server certificate" >&2
  exit ${STATUS}
fi

cat <<INFO
Issued development server certificate for ${SERVICE}.
  Private Key : ${KEY_PATH}
  Certificate : ${CERT_PATH}
  CSR         : ${CSR_PATH}
INFO
