# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- `validate --ref URI=FILE` registers a schema that a `$ref` may point at; repeatable, and the URI can be left off when the file carries its own `$id`
- `validate --resolve-refs` reads references that were not registered from disk, resolved against the location of the schema file itself; only `file:` and relative references are followed

## [0.1.0](https://github.com/iwe-org/schematter/releases/tag/schematter-v0.1.0) - 2026-07-12

Initial release.
