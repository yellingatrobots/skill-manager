# `apply` never touches the network

`apply` must produce the same result from the same local inputs every time. If
it could fetch, a moving `main` could change deployed skills between runs.
`apply` is therefore offline and deterministic from manifest plus lock plus
store, and `update` is the only command that resolves refs and fetches.

## Consequences

On a fresh machine the lock references store content that is not there.
`apply` fails loudly rather than papering over it; an explicit `hydrate` step
fetches exactly what the lock pins — never resolving refs, so it stays
deterministic. Auto-hydrating inside `apply` was rejected as hiding a network
call inside deployment; committing the store into `~/etc` alongside the
manifest was rejected as vendoring other people's code into the config repo,
though it is the only fully hermetic option.
