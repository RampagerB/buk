# RPM spec for buk, intended for a COPR project.
#
# It packages the fully static musl binary from the GitHub release, so there is
# nothing to compile and no cargo vendoring is needed. Once you have created a
# COPR project (e.g. https://copr.fedorainfracloud.org/coprs/<you>/buk/), build
# with:
#
#   spectool -g -C SOURCES buk.spec        # fetch Source0 (needs rpmdevtools)
#   rpmbuild -ba buk.spec --define "_sourcedir $PWD/SOURCES"
#   copr-cli build <you>/buk buk-<version>-1.src.rpm
#
# and users install with:
#   sudo dnf copr enable <you>/buk && sudo dnf install buk
#
# Update per release: Version, Source0 (checksum is verified by rpmbuild only
# if you add it via spectool -g -d), %changelog.

Name:           buk
Version:        0.1.0
Release:        1%{?dist}
Summary:        Back up files and directories to a dated, mirrored backup root
License:        Apache-2.0
URL:            https://github.com/RampagerB/buk
Source0:        %{url}/releases/download/v%{version}/buk-%{version}-x86_64-unknown-linux-musl.tar.gz
ExclusiveArch:  x86_64

%description
buk backs up files and directories to a configurable backup root ($) using a
dated suffix, and lists, restores, and cleans them up. No config file, no
daemon. This package installs the fully static musl build from the upstream
GitHub release.

%prep
%setup -n buk-%{version}-x86_64-unknown-linux-musl

%build
# Prebuilt fully static musl binary; nothing to build.

%install
# LICENSE and README.md are deliberately NOT installed here: the %license and
# %doc macros in %files copy them from the build directory themselves.
install -Dm755 buk %{buildroot}%{_bindir}/buk

%files
%license LICENSE
%doc README.md
%{_bindir}/buk

%changelog
* Wed Sep 23 2026 Ray Bao <raybao27@gmail.com> - 0.1.0-1
- Initial COPR package (static musl build from the v0.1.0 GitHub release)
