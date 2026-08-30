# Command contract

## `apply`

Offline. Validates the manifest and materializes a complete generated tree
from manifest plus lock plus store, with local-path sources read directly from
their working trees. It writes neither lock nor store.

Before changing the deployed tree, `apply` verifies that each remote lock entry
matches the manifest's transport, URL, `ref`, and `dir`, and that every
selected remote skill has a lock entry. Drift aborts with an instruction to
run `update`; extra locked skills that are no longer selected are ignored. A
malformed manifest aborts; an invalid individual skill is skipped while valid
skills deploy, and the command exits non-zero.

## `update [<alias>] [--no-apply]`

Resolves and fetches every remote source, or one source when an alias is
provided, then atomically replaces the lock beside the manifest. Unpinned sources
move to the current tip of declared intent; pinned sources resolve to the same
coordinate. Successful updates invoke `apply` unless `--no-apply` is passed.

Selected sources are staged before the lock changes. A source resolution or
fetch failure leaves the lock unchanged and does not invoke `apply`. A
structurally invalid skill retains its previous valid lock entry when one
exists; other skills advance and deploy, and the command exits non-zero.

## `hydrate`

Fetches exactly the upstream coordinates recorded in the lock and populates
missing store entries without resolving new versions. Every fetched skill is
verified against its locked content digest. For a well-known source, it simply
downloads the locked archive URL and compares the archive with the locked SHA;
a mismatch fails with an instruction to run `update`. Successful immutable
additions remain if another fetch fails; all failures are reported and the
command exits non-zero. `hydrate` never invokes `apply`.

## `add <alias>:<skill>...`

Adds one or more skill selections under existing source aliases, preserving
manifest formatting, then invokes `apply` once. It does not create sources.
Before editing either manifest or lock, it confirms every requested skill
exists and resolves only the affected remote aliases; local additions need no
resolution. Any preflight failure leaves both files unchanged. If a source
omits `skills` and therefore already selects all skills, `add` is a reported
no-op.

## `remove <alias>:<skill>...`

Removes one or more explicit skill selections, preserving manifest formatting,
then invokes `apply` once. It does not remove sources. Removing one skill from
a source that omits `skills` is an error because the manifest has no “all
except” representation.

## `info [<alias>:<skill>] [--full] [--json]`

Lists every deployed skill and concise provenance; a namespaced name narrows it
to one. `--full` also reports where this machine keeps things — manifest, lock,
store, deployed tree, the generation `current` points at, and each target — and
every provenance field per skill. Local-path sources are derived from the
manifest and current working tree even though they have no lock entry. `--json`
always provides the complete machine-readable
`{ "locations": {…}, "skills": […] }` object used by `sm:provenance`.

## `verify`

Read-only full convergence check. It verifies referenced store digests,
generated output, local-path sources against their working trees, the current
generation, target symlinks, and the absence of stale or foreign deployed
entries. Historical store entries not referenced by the current lock are
ignored.

## Concurrency

V1 adds no inter-process coordination. Callers are responsible for not running
mutating commands concurrently; introduce locking only in response to a real
failure mode.
