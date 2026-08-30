# Use the system Git client

Git and GitHub sources are resolved and fetched by invoking the installed
`git` executable into a temporary checkout. Explicit Git and SSH URLs are
preserved; GitHub shorthand is normalized to HTTPS. Submodules and Git LFS are
not initialized in v1.

## Considered Options

A Rust Git implementation would make the executable more self-contained but
would duplicate or bypass the user's SSH configuration, credential helpers,
and established private-repository authentication. It would also add a large
native or pure-Rust dependency surface for behavior the system client already
provides.

## Consequences

`git` is a runtime dependency for Git-backed sources, and a missing executable
must produce a direct actionable error. Fetching is hidden behind one internal
source adapter so command orchestration and tests do not depend directly on
process invocation.
