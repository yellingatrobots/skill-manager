# The store is content-addressed, not a git repository

Pristine upstream content lives in an immutable content-addressed store
(`store/<sha256>/`) rather than in a tool-owned git repository with one
commit per fetch. Content-addressing makes `verify` a rehash rather than a
`git status`, and makes the store genuinely immutable — a git working tree is
mutable by anything that walks past it, whereas a dirtied content-addressed
entry simply is not that entry any more.

## Considered Options

A tool-owned git repo would give history, cross-update diffs and rollback for
free, and `update --diff` is the one feature `npx skills` cannot offer. It is
rejected for now because none of those are v1 commands, and it would pay a
git dependency for surface area that is not exposed. If `update --diff` ships,
revisit this.

Bare mirrors of each upstream were rejected outright: tarball and well-known
sources are not git and would need a second mechanism anyway.

## Consequences

A "version" is two things, not one, and the lock records both: the upstream
coordinate (a commit SHA, a tarball digest) for provenance and for humans,
and the content digest of what was extracted for identity and change
detection. The existing `npx skills` lock already splits this way in practice
— git sources carry a folder hash, the well-known source carries only a
digest. For a local-path source the content digest is the only honest
version; `git describe --dirty` is a human annotation, since `-dirty` is the
same string for every distinct dirty state.
