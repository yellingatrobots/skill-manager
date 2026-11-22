# `apply` writes only the deployed tree

> Amended by ADR-0014: `apply` may write store entries the lock already
> references when it hydrates.

`update` writes the store and the lock; `hydrate` writes the store; `apply`
writes neither, only the generated skill directories. Anything `apply` writes
back into the config repo would leave `~/etc` permanently dirty, and anything
it wrote into the store would accumulate one entry per edit.

## Consequences

A local-path source has no place in the store or the lock: it is materialized
straight from its working tree on every `apply`, and `verify` compares the
deployed tree against that working tree rather than against a digest. This is
what makes editing a self-authored skill a matter of running `apply`, with no
fetch step in between. The rejected alternative — hashing the working tree
into the store on every `apply` — would keep one uniform code path across all
source types, at the cost of making `apply` a store-and-lock writer.
