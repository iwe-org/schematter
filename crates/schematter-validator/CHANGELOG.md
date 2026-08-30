# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- `$ref` keyword in the document schema dialect, at document, section, block and item level, together with top-level `$id` and `$defs`; `DocumentSchema` gained the matching `id` and `defs` fields
- `resolve` module with `CompileOptions` — register schemas by URI, set a base URI, attach a resolver — plus the `Resolver` trait and the `ResolveError` alias
- `compile_schema_with(source, options)` compiles a schema and resolves its external references through the registered schemas and, on a miss, the resolver

### Changed
- `$ref` inside `frontmatter` now resolves against the same registered schemas and resolver; previously any reference that did not start with `#` was rejected

## [0.1.0](https://github.com/iwe-org/schematter/releases/tag/schematter-validator-v0.1.0) - 2026-07-12

Initial release.
