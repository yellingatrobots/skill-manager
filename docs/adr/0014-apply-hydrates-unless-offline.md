# `apply` hydrates missing store entries unless `--offline`

ADR-0004 kept `apply` off the network so a moving `main` could not change
deployed skills between runs. Hydration cannot do that: it fetches only the
upstream coordinates the lock pins, resolves no refs, and refuses content that
does not match the locked digest. A hydrating `apply` therefore deploys the
same bytes as an offline one. Making it a separate step bought only the
absence of a network call, at the cost of a second command on every fresh
machine and an error that tells you to run it.

`apply` now fetches the locked content of every *selected* remote skill whose
store entry is absent, then deploys. `apply --offline` keeps the old
contract: no network, and a missing entry aborts. `hydrate` stays as an
explicit prefetch, for example before going offline.

## Considered Options

Opt-in `apply --hydrate` with an offline default was rejected: the strict
default only pays off where `apply` runs without network, such as an
activation script, and those callers can pass `--offline`.

## Consequences

Amends ADR-0004: `update` is still the only command that *resolves*; `apply`
may now *fetch*, but only what the lock pins. Amends ADR-0005: `apply` may
write the store, but only entries the lock already references, so it never
accumulates one entry per edit and still never writes the lock.

A hydrate failure aborts `apply` before the deployed tree changes, listing
every failure; a missing store entry was already fatal. Locked skills the
manifest no longer selects are not fetched.
