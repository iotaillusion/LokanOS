# Security Policy

## Supported Versions

LokanOS follows phased delivery. During Phase 0, security issues should still be reported so they can be triaged before subsequent phases are released.

## Reporting a Vulnerability

- Email: security@lokan.example.com
- Provide detailed reproduction steps, affected components, and potential impact.
- Encrypt sensitive reports using our PGP key (to be published in a later phase).

We acknowledge reports within 2 business days and aim to provide remediation guidance within 7 business days.

## Development PKI

For local development and integration testing we maintain helper scripts under
`security/pki/dev/` that generate a throw-away mutual TLS hierarchy. These
artifacts are **not** production-grade and must never be deployed beyond a
developer workstation. Each script makes a best effort to avoid overwriting
existing keys so teams can rotate the materials whenever needed.
