# OTA Bundle Layout

The updater expects every over-the-air (OTA) bundle to follow a fixed directory
structure. Bundles are directories (or archive roots) with the following
contents:

```
<bundle>/
├── manifest.json
├── images/
│   ├── boot.img
│   └── rootfs.img
└── sig/
    ├── sha256sum
    └── signature.pem
```

* `manifest.json` — metadata about the bundle and the slot it targets.
* `images/` — payload images. The updater currently expects at least
  `rootfs.img` and `boot.img`; additional components can be added as long as
  they are declared in the manifest.
* `sig/sha256sum` — deterministic list of component checksums in the format
  produced by `sha256sum` (`<digest><two spaces><relative path>`).
* `sig/signature.pem` — Ed25519 signature over the exact `sig/sha256sum`
  content. The signature file is encoded as PEM with the label
  `"ED25519 SIGNATURE"`.

## Manifest schema

`manifest.json` is UTF-8 JSON with the following fields:

| Field | Type | Description |
| ----- | ---- | ----------- |
| `version` | string | Semantic or build version of the OTA bundle. |
| `build_sha` | string | Source control identifier that produced the bundle. |
| `created_at` | string (RFC3339) | Timestamp the bundle was assembled. |
| `target_slot` | string (`"A"` or `"B"`) | Slot that must receive the images. |
| `components` | array | List of image components that make up the bundle. |

Each `components[]` entry is an object with the following members:

| Field | Type | Description |
| ----- | ---- | ----------- |
| `name` | string | Human readable component label (e.g. `"rootfs"`). |
| `path` | string | Relative path to the component within the bundle. |
| `sha256` | string | Lowercase hex SHA-256 digest of the component payload. |

The signing tools in `scripts/ota` will recompute and inject the checksum
values before signing. The updater service validates the manifest, recomputes
all component digests, verifies they match `sig/sha256sum`, and checks the
Ed25519 signature using the configured public key before staging an update.
