# Research Index — buffa-reflect

Background research feeding into `specs/buffa-reflect-*.md`.

| Document | Scope |
| --- | --- |
| [prost-reflect-architecture.md](./prost-reflect-architecture.md) | Deep-dive of `andrewhickman/prost-reflect`: descriptor pool, dynamic message, derive, build-time integration. The reference implementation we want to mirror. |
| [buffa-architecture.md](./buffa-architecture.md) | Deep-dive of `anthropics/buffa`: workspace layout, schema/wire model, codegen pipeline, existing reflection hooks, what is intentionally absent. |
| [gap-analysis.md](./gap-analysis.md) | Synthesis. Side-by-side mapping of prost-reflect concepts to buffa equivalents, integration strategy, and the open design questions that the spec must close. |

All sources cited use paths under `vendors/prost-reflect/` and `vendors/buffa/` — the two git submodules pinned at the commits used for this research.
