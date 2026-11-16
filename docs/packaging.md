# Building the packages

The packages offered with each release are built with one script, `packaging/build-packages.sh`. You can
run it yourself to check that a published package really comes from the published source code, or to build
packages from your own changes.

## What you need

- Linux on x86_64
- [podman](https://podman.io/), running without root
- about 10 GB of free disk space and an internet connection for the first run

Nothing else has to be installed: compilers and packaging tools live in the build containers.

## Running the build

In the directory of the source code:

```bash
packaging/build-packages.sh
```

The script builds the last commit and refuses to run if there are uncommitted changes. To build the files
as they are in your working directory, add `--worktree`. To build only some packages, add for example
`--only deb,arch` (known names: `deb`, `rpm`, `arch`).

What it does:

1. It packs the source code into `celata-<version>.tar.gz`.
2. It prepares one build image per package format (this step installs compilers and tools and needs the
   network).
3. It downloads all third-party crates once with `cargo vendor` and packs them into
   `celata-<version>-vendor.tar.zst` (this step needs the network, too).
4. It builds each package in its container **without any network access**, runs the tests of Celata and
   checks the result with the linter of the distribution (`lintian`, `rpmlint`, `namcap`). An error stops
   the build.
5. It writes everything to `dist/` together with `SHA256SUMS`.

| File in `dist/` | For | Built in |
|---|---|---|
| `celata_<version>-1_amd64.deb` | Debian 13, Ubuntu 24.04 and newer | Ubuntu 24.04 with Rust from rustup, because the Rust of Debian 13 and Ubuntu 24.04 is too old |
| `celata-<version>-1.x86_64.rpm` | Fedora 44 and 45 | Fedora 44 with the Rust of Fedora. The package carries no distribution tag; built on the oldest supported release, it installs on the newer ones |
| `celata-<version>-1-x86_64.pkg.tar.zst` | Arch Linux | Arch Linux with `makepkg` |
| `PKGBUILD`, `celata.install` | Arch Linux, to build the package yourself | – |
| `celata-<version>.tar.gz` | the source code that was built | – |
| `celata-<version>-vendor.tar.zst` | all third-party crates | – |
| `SHA256SUMS` | checksums of all files above | – |

## Checking a download

Download `SHA256SUMS` from the release page into the same directory as the package and run:

```bash
sha256sum -c SHA256SUMS --ignore-missing
```

Every file you downloaded must be reported as `OK`. The checksums protect against damaged downloads. They
do not prove who built the files; if you need that certainty, build the packages yourself as described
above and compare. The packages are not signed: `dnf` therefore reports that it skipped the OpenPGP
checks, and `apt` installs a local `.deb` without checking a signature anyway.

## What every package contains

| Path | Content |
|---|---|
| `/usr/bin/celata` | the program |
| `/usr/lib/systemd/system/celata-hide.me.service` | the service unit; not started and not enabled by the package |
| `/usr/lib/sysusers.d/celata.conf` | creates the group `celata` |
| `/usr/share/applications/celata.desktop` | entry in the application menu |
| `/usr/share/icons/hicolor/scalable/apps/celata.svg` | icon |
| `/usr/share/metainfo/io.github.derhouwer.celata.metainfo.xml` | description for software centers |
| documentation and license directories of the distribution | `README.md`, `CHANGELOG.md`, `docs/`, `LICENSE`, `THIRD-PARTY-LICENSES.md` |

No package contains the hide.me client, credentials, or files in `/etc`, `/opt` or a home directory.

Installing a package never starts or enables the service and adds no user to the group. Updating never
stops or restarts a running service. Removing stops and disables the service; the group stays, as the
packaging rules of the distributions demand.

The Arch package installs a variant of the service unit for the file locations of the AUR package
`hide-client` (`/usr/bin/hide.me`, `/etc/hide.me`). The other packages expect the client in `/opt/hide.me`,
where its own installer puts it.

## Building on Arch Linux without the script

`PKGBUILD` and `celata.install` are in `packaging/arch/`. Put them into an empty directory together with
`celata-<version>.tar.gz` and `celata-<version>-vendor.tar.zst` from the release page and run `makepkg -si`.
If the two archives are missing, `makepkg` downloads them from the release page.

## Package repositories

Celata is not yet available from a package repository, so updates are not automatic. The files in
`packaging/` are written in the formats that Fedora COPR (`packaging/rpm/celata.spec`) and the AUR
(`packaging/arch/PKGBUILD`) accept; publishing there may follow in a later version.
