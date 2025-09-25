# Contributing to LokanOS

Thanks for your interest in contributing! This document describes how to participate during Phase 0 and establishes coding standards that will be enforced in later phases.

## Getting Started
- Fork or clone the repository and ensure you are on the latest `work` branch.
- Install the Rust toolchain specified in `rust-toolchain.toml`.
- Run `make help` to discover common workflows.

## Development Workflow
1. Create focused commits with descriptive messages.
2. Run `make fmt lint test` before submitting changes.
3. Update documentation alongside code changes.

## Coding Standards
- Use Rust 2021 edition and follow `rustfmt` defaults.
- Deny compiler warnings in CI (`cargo clippy -- -D warnings`).
- Keep functions small and composable; prefer explicit types over inference in public APIs.
- Document all public items with `///` comments.

## Commit Message Guidelines
- Use the Conventional Commits format (e.g., `feat:`, `fix:`, `chore:`).
- Reference related issues or phases when applicable.

## Code Review
- Reviews focus on correctness, clarity, and adherence to the spec captured in `docs/spec.md`.
- Address review feedback promptly and follow up with additional tests when required.

Thank you for helping shape LokanOS!
