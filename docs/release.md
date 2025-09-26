# Release Compliance Checklist

The following steps help ensure every LokanOS release captures basic supply-chain metadata.

## Software Bill of Materials (SBOM)

- Run `make sbom` to produce a CycloneDX JSON SBOM at `dist/lokanos.sbom.json`.
- If [Syft](https://github.com/anchore/syft) is installed it will be used automatically; otherwise a minimal stub SBOM is emitted so downstream tooling always receives a valid document.
- Verify the SBOM is archived alongside the OTA bundle when packaging releases.

## Build Attestation

- Run `make attest` to generate a provenance statement at `dist/lokanos.att.json`.
- The attestation records the repository commit, UTC build timestamp, builder identity, and the list of tracked source inputs used for the build.

## OTA Packaging

- Run `make package` to create the OTA bundle. This target automatically regenerates the SBOM and attestation before invoking `os/images/build.sh` so the resulting `dist/` directory contains matching `*.sbom.json` and `*.att.json` artifacts for the release.
