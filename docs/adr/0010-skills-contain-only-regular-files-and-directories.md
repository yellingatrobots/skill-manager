# Skills contain only regular files and directories

Discovery and ingestion accept only regular files and directories inside a
skill. They never follow links. Symlinks, hardlinks, device nodes, sockets,
absolute archive paths, and paths containing a `..` traversal are rejected.
Archive extraction validates a path before writing it.

## Considered Options

Preserving links would support unusual skill layouts but creates two problems:
a link can escape the skill root and expose host files, and the same apparent
skill can hash or deploy differently depending on its surrounding filesystem.
Resolving links during ingestion merely hides the escape and silently copies
content the source did not visibly contain.

No currently authored or installed skill uses an internal symlink, so the
strict policy has no migration cost today.

## Consequences

The content digest and deployed tree depend only on bytes physically inside
the skill directory. A source that needs shared files must copy them into each
skill rather than link to them.
