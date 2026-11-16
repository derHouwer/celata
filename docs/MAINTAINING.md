# How Celata is maintained

Celata is developed and maintained by one person in their spare time. This page explains how new versions
come about, what every version is checked for, and what to keep in mind when the hide.me client is updated.
Feedback, bug reports and ideas are welcome, see [CONTRIBUTING.md](../CONTRIBUTING.md).

## Versions

- Version numbers follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html). What changed in each
  version is listed in [CHANGELOG.md](../CHANGELOG.md).
- Every version is published on the "Releases" page of the repository, with the packages and a file
  `SHA256SUMS`. How to check a download is described in
  [docs/packaging.md](packaging.md#checking-a-download).
- A published version is never changed afterwards. Fixes come as a new version.

## What every version is checked for

- All automated tests pass, and the usual Rust tools report nothing: `clippy` (code quality), `rustfmt`
  (formatting), `cargo audit` (known vulnerabilities in third-party code) and `cargo deny` (licenses and
  origin of third-party code).
- The packages are built from the published source code in containers without network access, then
  installed and started once on each supported system (see [Tested on](../README.md#tested-on)).
- Some principles hold for every version: Celata runs without administrator rights and never asks for a
  password; it contains no `unsafe` code; every text exists in all languages (English and German); text and
  background colours keep a contrast of at least 4.5 : 1; nothing is taken from the hide.me client or from
  hide.me.
- The state of the security review is described under [Status](../README.md#status) in the README and in
  [SECURITY.md](../SECURITY.md).

## The hide.me client

Celata 0.8.1 is tested with version 0.9.14 of the hide.me client. The client does not announce changes of
its control interface, and it silently ignores settings it does not know. Therefore:

- Celata shows a notice when the service reports a version other than the tested one.
- After an update of the client, connect, switch the server and disconnect once. If something does not work
  as before, please [report it](../CONTRIBUTING.md#reporting-a-bug) and name both versions.
- Every new version of Celata names the client version it was tested with, in the README and in the
  changelog.
- The filters of hide.me stay hidden until a client version that applies them has been tested, see
  [Known limitations](../README.md#known-limitations).

## Building it yourself

How to build Celata from source is described in [Building from source](../README.md#building-from-source),
how the packages are built in [docs/packaging.md](packaging.md), and the layout of the code in
[docs/ARCHITECTURE.md](ARCHITECTURE.md).
