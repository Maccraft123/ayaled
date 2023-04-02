# Maintainer: Maya Matuszczyk <maccraft123mc@gmail.com>
pkgname=ayaled
pkgver=0.1.0
pkgrel=1
makedepends=('cargo')
arch=('x86_64')

prepare() {
  cargo fetch --locked --target "$CARCH-unknown-linux-gnu"
}

build() {
  export RUSTUP_TOOLCHAIN=stable
  export CARGO_TARGET_DIR=target
  cargo build --frozen --release --all-features
}

check() {
  export RUSTUP_TOOLCHAIN=stable
  cargo test --frozen --all-features
}

package() {
  install -Dm0755 -t "$pkgdir/usr/bin/" "target/release/$pkgname"
  mkdir -p "$pkgdir/etc/systemd/system"
  install -m755 "$srcdir/ayaled.service" "$pkgdir/etc/systemd/system/ayaled.service"
}
