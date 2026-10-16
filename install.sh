#!/bin/sh
set -eu
cd "$(dirname "$0")"
cargo build --release --locked --offline
mkdir -p "$HOME/.local/bin" "$HOME/.local/lib/spotlight" "$HOME/.local/share/applications" "$HOME/.local/share/icons/hicolor/scalable/apps"
install -m 755 target/release/spotlight "$HOME/.local/lib/spotlight/spotlight"
if [ -f /usr/include/KPim6/AkonadiCore/Akonadi/ItemFetchJob ]; then
    sh native/build-pim.sh
    install -m 755 target/native/spotlight-pim "$HOME/.local/lib/spotlight/spotlight-pim"
fi
install -m 644 assets/UNICODE-LICENSE.txt "$HOME/.local/lib/spotlight/UNICODE-LICENSE.txt"
install -m 644 assets/spotlight.svg "$HOME/.local/share/icons/hicolor/scalable/apps/spotlight.svg"
cat > "$HOME/.local/bin/spotlight" <<'EOF'
#!/bin/sh
# Software rendering avoids a GPU context for this small launcher.
export GSK_RENDERER=cairo
exec "$HOME/.local/lib/spotlight/spotlight" "$@"
EOF
chmod 755 "$HOME/.local/bin/spotlight"
cat > "$HOME/.local/share/applications/spotlight.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=Spotlight
Comment=Search apps, files, clipboard, snippets, and commands
Exec="$HOME/.local/bin/spotlight"
Icon=spotlight
Terminal=false
Categories=Utility;
StartupNotify=true
StartupWMClass=local.spotlight.Spotlight
X-KDE-Shortcuts=Meta+Space
EOF
printf 'Installed Spotlight. Launch with %s/.local/bin/spotlight\n' "$HOME"
