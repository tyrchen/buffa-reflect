# Changelog

All notable changes to this project will be documented in this file. See [conventional commits](https://www.conventionalcommits.org/) for commit guidelines.

---
## [buffa-reflect-v0.2.0] - 2026-05-07

### Documentation

- rewrite README with Phase 2 guide and add runnable examples - ([1524741](https://github.com/tyrchen/buffa-reflect/commit/152474177d782249560d416aba2de56a07cd5198)) - Tyr Chen

### Miscellaneous Chores

- init commit - ([159af02](https://github.com/tyrchen/buffa-reflect/commit/159af029845911f72dcaaa100def3e5064d2f62e)) - Tyr Chen
- add specs - ([1cb0647](https://github.com/tyrchen/buffa-reflect/commit/1cb064759b847aab70824af88002de46b8af6528)) - Tyr Chen
- remove unused deps - ([ed3cbf7](https://github.com/tyrchen/buffa-reflect/commit/ed3cbf731d36617a63dc5c4a18e69da7e2ddc95e)) - Tyr Chen
- prepare workspace for crates.io publish - ([81dc84f](https://github.com/tyrchen/buffa-reflect/commit/81dc84fa816cffae7ac109498ca5ef50a60712c5)) - Tyr Chen

### Other

- M2 + M3: runtime tests and ReflectMessage derive macro

- 11 runtime integration tests cover the descriptor pool: lookup by FQN,
  field/oneof/Kind resolution, JSON-name camelCase derivation, packed-encoding
  inference, dangling type_name, duplicate types, reserved field numbers,
  proto3 enum-zero, and relative-name C++ scoping.
- The proc-macro derive parses #[buffa_reflect(...)] attributes with two
  expansion shapes:
    * descriptor_pool = "<expr>" — direct lookup against a user pool.
    * file_descriptor_set_bytes = "<expr>" — lazy OnceLock-backed pool.
- The derive accepts multiple message_name attributes and picks the longest,
  which is the FQN-most-specific value. This is required because buffa-build's
  type_attribute matching is segment-aware *prefix* matching: a nested
  message Inner of Outer receives both Outer's and Inner's per-FQN
  attributes, and the derive must pick the right one.
- 3 derive integration tests: bytes form, pool form, longest-wins lookup.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com> - ([1be96ca](https://github.com/tyrchen/buffa-reflect/commit/1be96cae99689564e099e4c0403971338293ca14)) - Tyr Chen
- buffa-reflect-build builder + integration tests - ([ac29739](https://github.com/tyrchen/buffa-reflect/commit/ac29739ba187e97f3415cadb9113682873706e58)) - Tyr Chen
- M5 + M6: end-to-end example app, Makefile targets, README

- apps/example demonstrates the full pipeline:
  * proto/acme/api/v1/library.proto (top-level msg, nested msg twice deep,
    enum, oneof, proto3 optional, map<string,string>)
  * build.rs drives Builder with file_descriptor_set_bytes
  * main.rs walks every field on Library / Book / Excerpt by descriptor,
    showing kind, cardinality, presence, and oneof membership.
- Makefile targets: build / test / lint / verify / example.
- Root README rewritten for the actual workspace layout and Quick start.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com> - ([f3fde4c](https://github.com/tyrchen/buffa-reflect/commit/f3fde4c6e4d7c67ad848cc21504cdcd050ded650)) - Tyr Chen
- address Phase 1 review findings + cross-impl equivalence test - ([feb89f1](https://github.com/tyrchen/buffa-reflect/commit/feb89f17c2ea8d7777e4530a10d66fee56f20bd6)) - Tyr Chen
- Phase 2 design — DynamicMessage, JSON, text, gRPC, view reflection - ([bc36ed4](https://github.com/tyrchen/buffa-reflect/commit/bc36ed40cfd4d1fc45b8ced117679d479f4c94b9)) - Tyr Chen
- revise Phase 2 design after auditing prost-reflect source - ([b5cce2a](https://github.com/tyrchen/buffa-reflect/commit/b5cce2a6221f9d9912f3c21be8a918e699bdfa22)) - Tyr Chen
- Phase 2a M1-M6: DynamicMessage core

Implements DynamicMessage (encode/decode/get/set/has/clear), Value /
MapKey value model, BTreeMap-based field storage with unknown-field
preservation and oneof bookkeeping, eager default-value parsing at
pool-build time (M3), wire encode/decode dispatch (M4/M5), and the
ReflectMessage::transcode_to_dynamic default impl (M6).

Gated behind the new default-on dynamic cargo feature; opt-out via
default-features = false continues to compile.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com> - ([cdb577e](https://github.com/tyrchen/buffa-reflect/commit/cdb577e252f4c0d303708e21674be63ead05d3be)) - Tyr Chen
- Phase 2b — proto3 JSON via serde - ([cb904f1](https://github.com/tyrchen/buffa-reflect/commit/cb904f10381aded788d5e860b5e50ae1dd6f9c0f)) - Tyr Chen
- Phase 2c — textproto encode/decode - ([ba07ac1](https://github.com/tyrchen/buffa-reflect/commit/ba07ac119c5e187e3723ab5003641df0ea6de313)) - Tyr Chen
- Phase 2e — view-type reflection - ([bb9d6a0](https://github.com/tyrchen/buffa-reflect/commit/bb9d6a0254d93eb3d2b5452ee2e998d266b724e6)) - Tyr Chen
- Phase 2d — gRPC server reflection - ([f8d9e2d](https://github.com/tyrchen/buffa-reflect/commit/f8d9e2d05b4aac9eee2d29a47819eee7818bfbc9)) - Tyr Chen
- address Phase 2 critical and high-impact findings - ([7cdd339](https://github.com/tyrchen/buffa-reflect/commit/7cdd3398c454adb5446c35f79bbccbc3a64eef80)) - Tyr Chen
- mark Phase 2 as shipped - ([8650776](https://github.com/tyrchen/buffa-reflect/commit/8650776ac5990ff52e0e0be503dd5bd82be09af9)) - Tyr Chen
- install protoc for example build - ([ffbe4f9](https://github.com/tyrchen/buffa-reflect/commit/ffbe4f9e449cc63d35c26ed4e5c81a04d59436c9)) - Tyr Chen
- update buffa version and bump verion - ([129f15f](https://github.com/tyrchen/buffa-reflect/commit/129f15fa911899f4002def19b24642107f248065)) - Tyr Chen

<!-- generated by git-cliff -->
