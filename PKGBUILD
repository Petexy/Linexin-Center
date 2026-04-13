# Maintainer: Petexy <https://github.com/Petexy>

pkgname=linexin-center
pkgver=3.0.0.r
pkgrel=3
pkgdesc='Linexin Center'
url='https://github.com/Petexy'
arch=('x86_64')
license=('GPL-3.0')
depends=(
  'python-gobject'
  'gtk4'
  'libadwaita'
  'python'
  'python-psutil'
  'python-distro'
)

package() {
    cd "${srcdir}"

    find usr -type f | while IFS= read -r _file; do
        if [[ "${_file}" == usr/bin/* ]]; then
            install -Dm755 "${_file}" "${pkgdir}/${_file}"
        else
            install -Dm644 "${_file}" "${pkgdir}/${_file}"
        fi
    done
}
