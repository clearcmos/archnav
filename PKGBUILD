# Maintainer: Nicholas Bedros <nicbedros@gmail.com>
pkgname=archnav
pkgver=0.1.0
pkgrel=1
pkgdesc='Fast keyboard-centric file navigator for KDE Wayland'
arch=('x86_64')
url='https://github.com/clearcmos/arch-nav'
license=('MIT')
depends=(
    'qt6-base'
    'qt6-declarative'
    'qt6-quickcontrols2'
    'kio'
    'kservice'
    'kcoreaddons'
    'ffmpeg'
    'poppler'
    'xdg-utils'
    'dolphin'
    'systemsettings'
    'p7zip'
)
makedepends=(
    'rust'
    'cargo'
    'pkg-config'
)
source=("$pkgname-$pkgver.tar.gz")
sha256sums=('SKIP')

build() {
    cd "$pkgname-$pkgver"
    cargo build --release --locked
}

package() {
    cd "$pkgname-$pkgver"
    install -Dm755 "target/release/archnav" "$pkgdir/usr/bin/archnav"
    install -Dm644 "data/archnav.desktop" "$pkgdir/usr/share/applications/archnav.desktop"
    install -Dm644 "archnav.svg" "$pkgdir/usr/share/icons/hicolor/scalable/apps/archnav.svg"
    install -Dm644 "LICENSE" "$pkgdir/usr/share/licenses/$pkgname/LICENSE" 2>/dev/null || true
}
