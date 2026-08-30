# The store is content-addressed, not a git repository

Pristine upstream content lives directly under the immutable content-addressed
store path `~/.local/share/skill-manager/store/sha256/<digest>/` rather than in
a tool-owned git repository with one commit per fetch. A new entry is written
to a temporary sibling, atomically renamed into place, then made read-only. Content-addressing makes `verify` a rehash rather than a
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

Unreferenced entries are retained indefinitely in v1. Skills are small, old
content aids rollback, and no demonstrated storage problem justifies a
destructive garbage-collection command. `verify` checks entries referenced by
the current lock and ignores historical unreferenced entries.
