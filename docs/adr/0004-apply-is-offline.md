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

`update` with no alias resolves every remote source; `update <alias>` resolves
one. Unpinned intent moves to its current tip and pinned intent resolves to the
same coordinate. A successful `update` invokes `apply` unless `--no-apply` is
passed.

An update stages all selected resolutions before replacing the lock. If any
source cannot resolve or fetch, the lock is unchanged and `apply` is not run;
content-addressed entries written during the attempt may remain. If one skill
is structurally invalid, its previous valid lock entry is retained when one
exists, the other skills advance and deploy, and the command exits non-zero.
