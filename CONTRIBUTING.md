# Contributing

Use focused branches and pull requests that solve one coherent problem. Commit
titles use `type: concise change`, such as `feat: reserve channel queue slots`,
`fix: quarantine interrupted publications`, or `docs: explain provider setup`.

Describe the concrete problem, resulting behavior, checks performed and material
limitations. Keep titles useful and specific. Avoid ceremony, implementation
diaries and claims that fixture tests establish live provider behavior.

Before merging, run the relevant build, formatting, integration and browser
checks. A fresh independent reviewer must inspect the actual diff and surrounding
code. Consequential fixes require another fresh review. Prefer tests of real
behavior and service boundaries over units that repeat implementation details.

Keep the MIT license and attribution intact. Do not copy incompatible licensed
code into this repository. Research notes link to upstream sources; those
projects are not required publishing backends.

Never commit credentials, OAuth responses, session captures, account exports,
private content, `.env` files or test media. Use isolated fixtures for publishing
tests. Public test posts, paid API calls and changes to existing queues require
explicit owner authorization. Keep disposable artifacts out of Git and remove
them after verification. Preserve unrelated work and normal browser profiles.

Update documentation and the capability matrix alongside behavior. State which
checks were fixture-based, which used a real provider sandbox, and which remain
blocked. Keep branches integrated; merge only after checks and independent
review pass. A merge that deploys needs owner release review first.
