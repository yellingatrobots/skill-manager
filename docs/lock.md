# Lock

The committed lock is deterministic TOML at `~/etc/ai/skills.lock`.

```toml
version = 1

[sources.mp]
kind = "github"
url = "https://github.com/mattpocock/skills.git"
ref = "main"

[sources.mp.skills.handoff]
path = "skills/handoff"
upstream_name = "handoff"
coordinate = "git:<commit-sha>"
digest = "sha256:<content-digest>"
```

Remote source identity is recorded once under its alias. Resolution is stored
per skill because a structurally invalid update may retain that skill's last
valid coordinate while sibling skills advance. Keys are serialized in
deterministic alias and skill order. No timestamps are written.

Local-path sources have no lock entries: `apply` reads them directly from
their working trees. They are still first-class provenance records in
`skill-manager info`, derived from the manifest and current source content.
Their configured path is reported as `upstream_url`, their semantic content
digest as `upstream_coordinate`, and `info` may additionally show the resolved
absolute path.
