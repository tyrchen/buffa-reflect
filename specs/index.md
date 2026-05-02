# Specs Index

## Phase 1 — descriptor pool + build script (shipped)

| Spec | Type | Status |
| --- | --- | --- |
| [buffa-reflect-prd.md](./buffa-reflect-prd.md) | PRD | Shipped |
| [buffa-reflect-design.md](./buffa-reflect-design.md) | Design | Shipped |
| [buffa-reflect-impl-plan.md](./buffa-reflect-impl-plan.md) | Impl plan | Shipped |

## Phase 2 — `DynamicMessage`, JSON, textproto, gRPC reflection, view reflection (shipped)

| Spec | Type | Status |
| --- | --- | --- |
| [buffa-reflect-phase2-prd.md](./buffa-reflect-phase2-prd.md) | PRD (umbrella for 2a–2e) | Shipped |
| [buffa-reflect-dynamic-design.md](./buffa-reflect-dynamic-design.md) | Design — Phase 2a (`DynamicMessage` core) | Shipped |
| [buffa-reflect-dynamic-impl-plan.md](./buffa-reflect-dynamic-impl-plan.md) | Impl plan — Phase 2a | Shipped |
| [buffa-reflect-json-design.md](./buffa-reflect-json-design.md) | Design — Phase 2b (proto3 JSON) | Shipped |
| [buffa-reflect-text-design.md](./buffa-reflect-text-design.md) | Design — Phase 2c (textproto) | Shipped |
| [buffa-reflect-grpc-design.md](./buffa-reflect-grpc-design.md) | Design — Phase 2d (gRPC reflection) | Shipped |
| [buffa-reflect-views-design.md](./buffa-reflect-views-design.md) | Design — Phase 2e (view reflection) | Shipped |

Sub-phase ordering: 2a is foundational (gates 2b and 2c). 2d and 2e have no Phase 2a dependency and may ship in parallel or before 2a. The umbrella PRD has a flow diagram and effort estimates.

The Phase 2 specs were revised after a deep audit of the prost-reflect source under `vendors/prost-reflect/`. Key revisions captured at the end of each design doc (look for the "Decisions revised after the audit" section in `dynamic-design.md`).

## Background research

[docs/research/](../docs/research/index.md) — prost-reflect deep-dive, buffa deep-dive, and the gap analysis that pinned down the integration approach.
