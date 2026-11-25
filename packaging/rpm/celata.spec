# SPDX-License-Identifier: GPL-3.0-only
# Copyright (C) 2026 derHouwer
#
# Builds Celata without network access against the vendored crates (Source1), see docs/packaging.md.

Name:           celata
Version:        0.9.0
# No distribution tag: one package serves every supported Fedora release. It is built on the oldest of them
# (Fedora 44). Its automatic dependencies name the libraries by their stable sonames, so dnf refuses to
# install it on a system where one is missing instead of installing a broken program.
Release:        1
Summary:        Unofficial graphical interface for the hide.me VPN command-line client

# Celata itself is GPL-3.0-only. The program is linked statically with third-party Rust crates and contains
# flags from flag-icons (MIT) and icons from Lucide (ISC); all of them are listed with their license texts
# in THIRD-PARTY-LICENSES.md.
License:        GPL-3.0-only
URL:            https://github.com/derHouwer/celata
Source0:        %{url}/archive/v%{version}/celata-%{version}.tar.gz
Source1:        celata-%{version}-vendor.tar.zst

ExclusiveArch:  x86_64

BuildRequires:  cargo-rpm-macros >= 26
BuildRequires:  pkgconfig(fontconfig)
BuildRequires:  systemd-rpm-macros
BuildRequires:  desktop-file-utils
BuildRequires:  appstream

# Libraries Celata loads only when it starts; the automatic dependency generator cannot see them.
Requires:       libwayland-client.so.0()(64bit)
Requires:       libwayland-cursor.so.0()(64bit)
Requires:       libwayland-egl.so.1()(64bit)
Requires:       libxkbcommon.so.0()(64bit)
Requires:       libEGL.so.1()(64bit)
Requires:       iproute
Requires:       systemd
# Lets members of the group celata start the service without a password (packaging/polkit/49-celata.rules).
Requires:       polkit
Requires:       tzdata
Requires:       hicolor-icon-theme
# Opening the donation page, and the libraries of the X11 session (enabled but untested).
Recommends:     xdg-utils
Recommends:     libX11.so.6()(64bit)
Recommends:     libX11-xcb.so.1()(64bit)
Recommends:     libXcursor.so.1()(64bit)
Recommends:     libXi.so.6()(64bit)
Recommends:     libxkbcommon-x11.so.0()(64bit)
Recommends:     libGL.so.1()(64bit)

%description
Celata shows the state of the VPN connection, lets you choose a location,
connect, switch and disconnect, and block the internet with the kill switch
of the hide.me client. It runs without administrator rights and talks to the
hide.me client through a local control socket.

Requires the hide.me command-line client (not included) and a hide.me
account. Wayland sessions are supported; X11 is enabled but untested.

Celata is an independent project. It is not affiliated with or endorsed by
hide.me.

%prep
%autosetup -n celata-%{version} -a 1
%cargo_prep -v vendor

%build
%cargo_build

%install
install -D -p -m 0755 target/rpm/celata %{buildroot}%{_bindir}/celata
install -D -p -m 0644 packaging/systemd/celata-hide.me.service %{buildroot}%{_unitdir}/celata-hide.me.service
install -D -p -m 0644 packaging/sysusers/celata.conf %{buildroot}%{_sysusersdir}/celata.conf
install -D -p -m 0644 packaging/polkit/49-celata.rules %{buildroot}%{_datadir}/polkit-1/rules.d/49-celata.rules
install -D -p -m 0644 packaging/desktop/celata.desktop %{buildroot}%{_datadir}/applications/celata.desktop
install -D -p -m 0644 ui/icons/celata.svg %{buildroot}%{_datadir}/icons/hicolor/scalable/apps/celata.svg
install -D -p -m 0644 packaging/desktop/io.github.derhouwer.celata.metainfo.xml \
    %{buildroot}%{_metainfodir}/io.github.derhouwer.celata.metainfo.xml

%check
%cargo_test
desktop-file-validate %{buildroot}%{_datadir}/applications/celata.desktop
appstreamcli validate --no-net %{buildroot}%{_metainfodir}/io.github.derhouwer.celata.metainfo.xml

# Installing never starts or enables the service. Removing stops and disables it. Updating leaves a running
# service alone: there is deliberately no %%postun with a restart.
%post
%systemd_post celata-hide.me.service

%preun
%systemd_preun celata-hide.me.service

%files
%license LICENSE THIRD-PARTY-LICENSES.md
%doc README.md CHANGELOG.md docs/*.md
%{_bindir}/celata
%{_unitdir}/celata-hide.me.service
%{_sysusersdir}/celata.conf
# The directory belongs to the package polkit.
%{_datadir}/polkit-1/rules.d/49-celata.rules
%{_datadir}/applications/celata.desktop
%{_datadir}/icons/hicolor/scalable/apps/celata.svg
%{_metainfodir}/io.github.derhouwer.celata.metainfo.xml

%changelog
* Thu Nov 26 2026 derHouwer <355941224+derHouwer@users.noreply.github.com> - 0.9.0-1
- The state of the connection in colour, an icon with a menu in the system tray and optional desktop notifications
- Locations by country, favorite locations and an automatic "Best location"
- Light and dark appearance
- Celata starts the hide.me service itself (rule for polkit in the package)
- Optional check for new versions (off by default)
- Setup script and Linux Mint 22 as a tested system
* Mon Nov 16 2026 derHouwer <355941224+derHouwer@users.noreply.github.com> - 0.8.1-1
- First public version
