#!/bin/sh
# Paquet .deb autonome de TextEdit, installable hors de Ferrite OS (GNOME, KDE, Xfce ;
# Debian 13, Ubuntu 24.04 ou plus récent, Kali…).
#
#   tools/deb-textedit.sh        compile en mode release puis écrit build/ferrite-textedit_VERSION_amd64.deb
#
# Installation sur l'autre machine : sudo apt install ./ferrite-textedit_VERSION_amd64.deb
#
# Contenu : le binaire ferrite-apps dans /usr/lib/ferrite-textedit (il contient aussi les
# autres applications, que TextEdit lance parfois, comme « Afficher dans Fichiers »), la
# commande /usr/bin/ferrite-textedit, son entrée de menu et son icône. Rien n'est écrit
# dans /usr/local : une image Ferrite qui a déjà TextEdit garde le sien.
set -eu
. "$(dirname -- "$0")/lib.sh"
priorite_basse
cd "$REPO"

command -v dpkg-deb >/dev/null 2>&1 || die "dpkg-deb absent (paquet dpkg)"

version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
[ -n "$version" ] || die "version introuvable dans Cargo.toml"

info "compilation (release)"
cargo build --release --locked -p ferrite-apps

# Version de la glibc demandée par le binaire : elle fixe les distributions compatibles.
glibc=$(objdump -T target/release/ferrite-apps | grep -o 'GLIBC_[0-9.]*' | sed 's/GLIBC_//' \
    | sort -uV | tail -n 1)
[ -n "$glibc" ] || die "version de la glibc introuvable"

root="$BUILD/deb-textedit"
rm -rf "$root"
mkdir -p "$root/DEBIAN" "$root/usr/bin" "$root/usr/lib/ferrite-textedit" \
    "$root/usr/share/applications" "$root/usr/share/icons/hicolor/scalable/apps" \
    "$root/usr/share/doc/ferrite-textedit"

install -m 755 target/release/ferrite-apps "$root/usr/lib/ferrite-textedit/ferrite-apps"
install -m 644 icons/SVG/app-text-editor.svg \
    "$root/usr/share/icons/hicolor/scalable/apps/ferrite-textedit.svg"

cat >"$root/usr/bin/ferrite-textedit" <<'EOF'
#!/bin/sh
# TextEdit de Ferrite OS : ouvre les fichiers donnés (ou une fenêtre vide).
exec /usr/lib/ferrite-textedit/ferrite-apps textedit "$@"
EOF
chmod 755 "$root/usr/bin/ferrite-textedit"

# Même nom que l'identifiant de la fenêtre (app_id) : GNOME relie ainsi la fenêtre à son
# icône et à son nom dans la barre des activités.
cat >"$root/usr/share/applications/ferrite-textedit.desktop" <<'EOF'
[Desktop Entry]
Type=Application
Name=TextEdit
GenericName=Éditeur de texte
Comment=Éditeur de texte de Ferrite OS, avec aperçu Markdown
Exec=ferrite-textedit %F
Icon=ferrite-textedit
Terminal=false
StartupWMClass=ferrite-textedit
MimeType=text/plain;text/markdown;text/x-log;text/csv;text/rust;application/toml;application/json;application/yaml;application/x-shellscript;text/x-python3;text/x-python;text/javascript;text/x-csrc;text/x-chdr;text/x-c++src;text/x-c++hdr;text/css;application/xml;
Categories=Utility;TextEditor;
Keywords=texte;éditeur;markdown;code;
EOF

cat >"$root/usr/share/doc/ferrite-textedit/copyright" <<'EOF'
TextEdit fait partie de Ferrite OS.
Polices intégrées : Inter et JetBrains Mono (licence SIL Open Font License 1.1).
EOF

# Droits habituels d'un paquet, quel que soit le masque (umask) de la session.
find "$root" -type d -exec chmod 755 {} +
find "$root/usr/share" -type f -exec chmod 644 {} +

size=$(du -sk "$root/usr" | cut -f1)
cat >"$root/DEBIAN/control" <<EOF
Package: ferrite-textedit
Version: $version
Section: editors
Priority: optional
Architecture: amd64
Maintainer: Ferrite OS <noreply@ferrite.invalid>
Installed-Size: $size
Depends: libc6 (>= $glibc), libgcc-s1, libgstreamer1.0-0, libgstreamer-plugins-base1.0-0, libwayland-client0, libwayland-egl1, libxkbcommon0, libegl1, libgl1, libx11-6, libx11-xcb1, libxcursor1, libxi6, libxrandr2, libxkbcommon-x11-0
Recommends: fonts-noto-color-emoji, xdg-utils
Description: Éditeur de texte de Ferrite OS
 TextEdit, l'éditeur de texte du bureau Ferrite OS : onglets, explorateur de
 dossier, coloration du code, aperçu Markdown, recherche et remplacement.
 Fonctionne sous Wayland et X11 (GNOME, KDE, Xfce…), avec sa propre barre du
 haut à la place de la barre de titre du système.
EOF

out="$BUILD/ferrite-textedit_${version}_amd64.deb"
dpkg-deb --root-owner-group --build "$root" "$out" >/dev/null
ok "$out ($(du -h "$out" | cut -f1), glibc $glibc ou plus récente)"
