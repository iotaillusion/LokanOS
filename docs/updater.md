# Updater Service

## Bundle verification

The updater stages OTA bundles by validating them on disk before any slot state
changes happen. Bundles must follow the layout documented in
[`os/images/layout/README.md`](../os/images/layout/README.md). During staging the
service performs the following checks:

1. Load `manifest.json` and confirm it contains `version`, `build_sha`,
   `created_at`, `target_slot`, and at least one component entry.
2. Reject component paths that escape the bundle (absolute paths or `..`).
3. Recompute SHA-256 digests for every component and ensure they match the
   values embedded in the manifest.
4. Load `sig/sha256sum`, parse the list of component digests, and verify it
   matches the manifest exactly.
5. Read `sig/signature.pem`, decode the Ed25519 signature, and verify it against
   the checksum file using the configured OTA public key. The default key lives
   at `security/pki/dev/ota/ota_signing_public.pem`, and can be overridden with
   the `UPDATER_OTA_PUBLIC_KEY` environment variable.

Any mismatch raises a staging error and prevents the bundle from being recorded
in updater state.

## Signing workflow (development)

Development keys live in `security/pki/dev/ota`. The private key must be kept on
build hosts only; the repository includes it for local testing, but production
should supply a different key pair.

Two helper scripts automate signing and verification:

* `scripts/ota/sign.sh <bundle_dir>` – Updates the manifest with freshly
  computed SHA-256 digests, writes `sig/sha256sum`, signs it with the configured
  private key, and emits `sig/signature.pem`. The script accepts optional
  private and public key paths: `sign.sh <bundle> <priv> <pub>`.
* `scripts/ota/verify.sh <bundle_dir>` – Recomputes the component checksums,
  checks them against the manifest and checksum file, and verifies the Ed25519
  signature with the public key (override with a second argument).

Example workflow:

```
$ tree bundle/
bundle/
├── images
│   ├── boot.img
│   └── rootfs.img
├── manifest.json
└── sig
    └── (empty)

$ scripts/ota/sign.sh bundle/
$ scripts/ota/verify.sh bundle/
```

The updater service uses the same verification logic before writing any staging
state. Custom build pipelines should either call the scripts or replicate their
logic to guarantee the service will accept the artifact.
