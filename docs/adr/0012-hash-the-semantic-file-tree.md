# Hash the semantic file tree

A skill's content digest is SHA-256 over its regular files in bytewise-sorted
UTF-8 relative-path order. The stream starts with a format marker and encodes,
for each file, its path length and bytes, executable bit, content length, and
content bytes. Length-prefixing makes the stream unambiguous without an
archive or serialization dependency.

The digest excludes timestamps, ownership, non-executable permission bits,
and directory entries. Empty directories therefore do not contribute to
identity or deployment. Non-UTF-8 paths are rejected.

## Considered Options

A normalized archive format would preserve more filesystem metadata but would
add archive rules and a dependency for metadata skills do not use. Hashing
only concatenated file bytes is simpler but ambiguous and would not detect
renames. Git tree hashes do not apply uniformly to tarball and local-path
sources.

## Consequences

The format marker versions the canonical stream independently from the lock
schema. Changing these rules requires a new marker and produces new content
digests; old store entries remain valid under their recorded digest.
