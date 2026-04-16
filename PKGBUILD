# Maintainer: Nicholas Bedros <nicbedros@gmail.com>
pkgname=archnav-git
pkgver=0.1.0.r0.0000000
pkgrel=1
pkgdesc='Fast keyboard-centric file navigator for KDE Wayland'
arch=('x86_64')
url='https://github.com/clearcmos/archnav'
license=('MIT')
depends=(qt6-base
         qt6-declarative
         kio
         kservice
         kcoreaddons
         ffmpeg
         poppler
         xdg-utils
         dolphin
         systemsettings
         p7zip)
makedepends=(git
             rust
             cargo
             pkg-config)
provides=('archnav')
conflicts=('archnav')
source=("$pkgname::git+https://github.com/clearcmos/archnav.git")
sha256sums=('SKIP')

pkgver() {
    cd "$pkgname"
    printf "0.1.0.r%s.%s" \
        "$(git rev-list --count HEAD)" \
        "$(git rev-parse --short HEAD)"
}

build() {
    cd "$pkgname"
    cargo build --release --locked
}

package() {
    cd "$pkgname"
    install -Dm755 "target/release/archnav" "$pkgdir/usr/bin/archnav"
    install -Dm644 "data/archnav.desktop" "$pkgdir/usr/share/applications/archnav.desktop"
    install -Dm644 "archnav.svg" "$pkgdir/usr/share/icons/hicolor/scalable/apps/archnav.svg"
}
