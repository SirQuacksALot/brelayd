# Maintainer: SirQuacksALot <sglass@hs-mittweida.de>

pkgname=brelayd
pkgver=0.1.0
pkgrel=1
pkgdesc="Config-driven daemon that runs commands on battery events (UPower/D-Bus)"
arch=('x86_64')
url="https://github.com/SirQuacksALot/brelayd"
license=('MIT')
makedepends=('rust')
source=("$pkgname-$pkgver.tar.gz::https://github.com/SirQuacksALot/brelayd/archive/refs/tags/v$pkgver.tar.gz")
sha256sums=('SKIP')

build() {
  cd "$pkgname-$pkgver"
  cargo build --release --locked
}

package() {
  cd "$pkgname-$pkgver"
  install -Dm755 target/release/brelayd "$pkgdir/usr/bin/brelayd"
  install -Dm644 config.example.toml "$pkgdir/usr/share/brelayd/config.example.toml"
  install -Dm644 contrib/brelayd.service "$pkgdir/usr/lib/systemd/user/brelayd.service"
  install -Dm644 LICENSE "$pkgdir/usr/share/licenses/$pkgname/LICENSE"
}
