# Agent instructions

`CONTEXT.md` holds the vocabulary, `docs/` the contracts, `docs/adr/` the
decisions. Read those before changing behaviour.

## Versioning

Bump the point (patch) version in `Cargo.toml` in **every** commit that touches
`src/`, and let `Cargo.lock` follow (`cargo build` rewrites it). Minor and major
bumps stay a deliberate call.

Consumers pin this repo as a flake input and see the version in the built
package name, so two revisions sharing a version are indistinguishable once
installed.
