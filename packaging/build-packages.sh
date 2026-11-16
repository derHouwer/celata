#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-only
# Copyright (C) 2026 derHouwer
#
# Builds the Celata packages in rootless podman containers and writes them to dist/:
#
#   celata_<version>-<n>_amd64.deb          Debian and Ubuntu, built on Ubuntu 24.04 with Rust from rustup
#   celata-<version>-<n>.x86_64.rpm         Fedora 44 and 45 (one package), built on Fedora 44 with the
#                                           Rust of Fedora
#   celata-<version>-<n>-x86_64.pkg.tar.zst Arch Linux, built with makepkg
#   PKGBUILD, celata.install                the Arch recipe itself
#   celata-<version>.tar.gz                 the source code that was built
#   celata-<version>-vendor.tar.zst         all third-party crates (cargo vendor)
#   SHA256SUMS
#
# Only two steps use the network: preparing the build images (installing compilers and tools) and
# downloading the third-party crates once. The builds themselves run without network access.
#
# Usage: packaging/build-packages.sh [--worktree] [--only deb,rpm,arch] [--pkgrel N]
#
#   --worktree   build the files as they are in the working directory, including uncommitted changes.
#                Without it the last commit is built and uncommitted changes are an error.
#   --only LIST  build only these packages (comma-separated)
#   --pkgrel N   package release number (default 1); only meant for testing an update
#
# See docs/packaging.md.

set -euo pipefail

RUST_TOOLCHAIN=1.99.1
# The .rpm is built on the oldest supported Fedora release and installs on the newer ones.
FEDORA_RELEASE=44
ALL_TARGETS=(deb rpm arch)

die() {
    echo "build-packages: $*" >&2
    exit 1
}

worktree=no
pkgrel=1
targets=("${ALL_TARGETS[@]}")
while [ $# -gt 0 ]; do
    case "$1" in
        --worktree) worktree=yes ;;
        --only)
            [ $# -ge 2 ] || die "--only needs a list"
            IFS=, read -r -a targets <<<"$2"
            shift
            ;;
        --pkgrel)
            [ $# -ge 2 ] || die "--pkgrel needs a number"
            pkgrel=$2
            shift
            ;;
        -h | --help)
            sed -n '4,27p' "$0" | sed 's/^# \{0,1\}//'
            exit 0
            ;;
        *) die "unknown option: $1" ;;
    esac
    shift
done
[[ $pkgrel =~ ^[1-9][0-9]*$ ]] || die "--pkgrel must be a positive number"
for target in "${targets[@]}"; do
    [[ " ${ALL_TARGETS[*]} " == *" $target "* ]] || die "unknown package: $target (known: ${ALL_TARGETS[*]})"
done
command -v podman >/dev/null || die "podman is required"

root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"
version=$(sed -n '/^\[workspace\.package\]/,/^\[/s/^version = "\(.*\)"$/\1/p' Cargo.toml)
[ -n "$version" ] || die "no version found in Cargo.toml"
name="celata-$version"
dist="$root/dist"
work="$dist/.work"
cache="$dist/.cache"

rm -rf "$work"
mkdir -p "$work/home" "$cache"
# Results of an earlier run must not end up in SHA256SUMS. With --only, the other packages stay.
if [ "${#targets[@]}" -eq "${#ALL_TARGETS[@]}" ]; then
    find "$dist" -maxdepth 1 -type f -delete
fi

# --- 1. Source archive ---------------------------------------------------------------------------------

source_archive="$work/$name.tar.gz"
if git -C "$root" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
    if [ "$worktree" = yes ]; then
        # Snapshot of the working directory through a temporary index: no commit, the real index stays.
        index=$(mktemp)
        cp "$(git rev-parse --git-path index)" "$index"
        GIT_INDEX_FILE="$index" git add -A
        tree=$(GIT_INDEX_FILE="$index" git write-tree)
        rm -f "$index"
    else
        [ -z "$(git status --porcelain)" ] || die "uncommitted changes; commit them or use --worktree"
        tree=HEAD
    fi
    SOURCE_DATE_EPOCH=$(git log -1 --format=%ct)
    git archive --format=tar --prefix="$name/" "$tree" | gzip -n >"$source_archive"
else
    # Not a git checkout (for example an unpacked source archive): take the directory as it is.
    SOURCE_DATE_EPOCH=$(stat -c %Y Cargo.lock)
    tar --sort=name --mtime="@$SOURCE_DATE_EPOCH" --owner=0 --group=0 --numeric-owner \
        --exclude=./target --exclude=./dist --exclude=./vendor --exclude=./.git \
        --transform "s,^\.,$name," -cf - . | gzip -n >"$source_archive"
fi
export SOURCE_DATE_EPOCH
echo "==> source: $name.tar.gz"

# --- 2. Build images (network) -------------------------------------------------------------------------

image() {
    echo "localhost/celata-build-$1"
}

build_image() {
    local target=$1
    echo "==> image: $(image "$target")"
    case "$target" in
        deb)
            podman build -q -t "$(image deb)" --build-arg "RUST_TOOLCHAIN=$RUST_TOOLCHAIN" -f - . <<'EOF' >/dev/null
FROM docker.io/library/ubuntu:24.04
ARG RUST_TOOLCHAIN
ENV DEBIAN_FRONTEND=noninteractive RUSTUP_HOME=/opt/rustup CARGO_HOME=/opt/cargo PATH=/opt/cargo/bin:$PATH
RUN apt-get update \
    && apt-get install -y --no-install-recommends build-essential debhelper pkgconf libfontconfig-dev lintian \
       ca-certificates curl zstd \
    && rm -rf /var/lib/apt/lists/*
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
    | sh -s -- -y --profile minimal --default-toolchain "$RUST_TOOLCHAIN" --no-modify-path \
    && chmod -R a+rX /opt/rustup /opt/cargo
EOF
            ;;
        rpm)
            podman build -q -t "$(image rpm)" --build-arg "RELEASE=$FEDORA_RELEASE" -f - . <<'EOF' >/dev/null
ARG RELEASE
FROM registry.fedoraproject.org/fedora:${RELEASE}
RUN dnf -y install rpm-build rpmlint cargo-rpm-macros rust cargo gcc pkgconf-pkg-config fontconfig-devel \
       systemd-rpm-macros desktop-file-utils appstream zstd \
    && dnf clean all
EOF
            ;;
        arch)
            podman build -q -t "$(image arch)" -f - . <<'EOF' >/dev/null
FROM docker.io/library/archlinux:latest
RUN pacman -Syu --noconfirm --needed base-devel rust namcap fontconfig ttf-dejavu wayland libxkbcommon libglvnd \
       iproute2 systemd hicolor-icon-theme zstd \
    && pacman -Scc --noconfirm
EOF
            ;;
    esac
}

# Runs the script from standard input in a build image. $1 = package, $2 = "online" or "offline" (no
# network at all), rest = further podman options. The script runs as the calling user, mapped into the
# container, never as root: makepkg refuses root, and tests about missing permissions need a normal user.
run_in() {
    local target=$1 network=()
    [ "$2" = online ] || network=(--network=none)
    shift 2
    podman run --rm -i "${network[@]}" --security-opt label=disable \
        --userns=keep-id --user "$(id -u):$(id -g)" -e HOME=/work/home \
        -e SOURCE_DATE_EPOCH -e "NAME=$name" -e "VERSION=$version" -e "PKGREL=$pkgrel" \
        -v "$work:/work" "$@" "$(image "$target")" bash -euo pipefail -s
}

for target in "${targets[@]}"; do
    build_image "$target"
done

# --- 3. Third-party crates (network, once per Cargo.lock) ----------------------------------------------

vendor_archive="$work/$name-vendor.tar.zst"
vendor_cached="$cache/vendor-$(sha256sum Cargo.lock | cut -c1-16).tar.zst"
if [ ! -f "$vendor_cached" ]; then
    echo "==> vendor: downloading the third-party crates"
    run_in "${targets[0]}" online <<'EOF'
rm -rf /work/vendor && mkdir /work/vendor && cd /work/vendor
tar -xzf "/work/$NAME.tar.gz"
cd "$NAME"
export CARGO_HOME=/work/vendor/cargo-home
cargo vendor --quiet --locked vendor >/dev/null
tar --sort=name --mtime="@$SOURCE_DATE_EPOCH" --owner=0 --group=0 --numeric-owner -cf - vendor \
    | zstd -19 -T0 -q -o "/work/$NAME-vendor.tar.zst"
rm -rf /work/vendor
EOF
    cp "$vendor_archive" "$vendor_cached"
else
    cp "$vendor_cached" "$vendor_archive"
fi
echo "==> vendor: $name-vendor.tar.zst"

# --- 4. Builds (no network) ----------------------------------------------------------------------------

build_deb() {
    run_in deb offline <<'EOF'
rm -rf /work/deb && mkdir /work/deb && cd /work/deb
tar -xzf "/work/$NAME.tar.gz"
cd "$NAME"
tar --zstd -xf "/work/$NAME-vendor.tar.zst"
cp -r packaging/debian debian
sed -i "1s/($VERSION-1)/($VERSION-$PKGREL)/" debian/changelog
dpkg-buildpackage -b -us -uc
cd ..
lintian --fail-on error --display-info "celata_${VERSION}-${PKGREL}_amd64.deb" | tee lintian.txt
EOF
    cp "$work/deb/celata_${version}-${pkgrel}_amd64.deb" "$dist/"
}

build_rpm() {
    run_in rpm offline <<'EOF'
top=/work/rpm
rm -rf "$top" && mkdir -p "$top/SOURCES" "$top/SPECS"
cp "/work/$NAME.tar.gz" "/work/$NAME-vendor.tar.zst" "$top/SOURCES/"
tar -xzf "/work/$NAME.tar.gz" -O "$NAME/packaging/rpm/celata.spec" >"$top/SPECS/celata.spec"
sed -i "s/^Release: .*/Release:        $PKGREL/" "$top/SPECS/celata.spec"
rpmbuild --define "_topdir $top" -bb "$top/SPECS/celata.spec"
rpmlint "$top/RPMS/x86_64/celata-$VERSION-$PKGREL.x86_64.rpm" | tee "$top/rpmlint.txt"
EOF
    cp "$work/rpm/RPMS/x86_64/celata-$version-$pkgrel.x86_64.rpm" "$dist/"
}

build_arch() {
    run_in arch offline <<'EOF'
rm -rf /work/arch && mkdir -p /work/arch && cd /work/arch
tar -xzf "/work/$NAME.tar.gz" --strip-components=3 "$NAME/packaging/arch/PKGBUILD" "$NAME/packaging/arch/celata.install"
sed -i "s/^pkgrel=.*/pkgrel=$PKGREL/" PKGBUILD
cp "/work/$NAME.tar.gz" "/work/$NAME-vendor.tar.zst" .
PKGEXT=.pkg.tar.zst makepkg -f --noconfirm
{ namcap PKGBUILD; namcap "celata-$VERSION-$PKGREL-x86_64.pkg.tar.zst"; } | tee namcap.txt
if grep -q ' E: ' namcap.txt; then
    echo "namcap reports errors" >&2
    exit 1
fi
EOF
    cp "$work/arch/celata-$version-$pkgrel-x86_64.pkg.tar.zst" "$work/arch/PKGBUILD" "$work/arch/celata.install" "$dist/"
    # The published PKGBUILD verifies the two archives it downloads.
    local source_sum vendor_sum
    source_sum=$(sha256sum "$source_archive" | cut -d' ' -f1)
    vendor_sum=$(sha256sum "$vendor_archive" | cut -d' ' -f1)
    sed -i -e "s/^sha256sums=('SKIP'\$/sha256sums=('$source_sum'/" -e "s/^            'SKIP')\$/            '$vendor_sum')/" "$dist/PKGBUILD"
    if grep -q "'SKIP'" "$dist/PKGBUILD"; then
        die "the checksums could not be written into dist/PKGBUILD"
    fi
}

for target in "${targets[@]}"; do
    echo "==> build: $target"
    case "$target" in
        deb) build_deb ;;
        rpm) build_rpm ;;
        arch) build_arch ;;
    esac
done

# --- 5. Results ----------------------------------------------------------------------------------------

cp "$source_archive" "$vendor_archive" "$dist/"
(cd "$dist" && find . -maxdepth 1 -type f ! -name SHA256SUMS -printf '%f\n' | LC_ALL=C sort | xargs sha256sum >SHA256SUMS)
echo "==> done: $dist"
cat "$dist/SHA256SUMS"
