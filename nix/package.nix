{
  lib,
  rustPlatform,
  runtimeShell,
  pkg-config,
  wrapGAppsHook4,
  gnome,
  gdk-pixbuf,
  webp-pixbuf-loader,
  gtk4,
  sqlite,
  gsettings-desktop-schemas,
  git,
  ripgrep,
  gzip,
  vips,
  ffmpeg,
  zbar,
  poppler-utils,
  python3,
  curl,
  xdg-utils,
}:

let
  # gdk-pixbuf keeps PNG and JPEG in the library, but WebP and the other
  # formats need a loaders cache that also covers the separate loader packages.
  gdkPixbufLoaders = gnome._gdkPixbufCacheBuilder_DO_NOT_USE {
    extraLoaders = [ webp-pixbuf-loader ];
  };
in
rustPlatform.buildRustPackage {
  pname = "spotlight";
  version = (builtins.fromTOML (builtins.readFile ../Cargo.toml)).package.version;

  src = lib.cleanSourceWith {
    src = ../.;
    filter = path: type:
      let name = baseNameOf (toString path);
      in !(type == "directory" && (name == ".git" || name == "result" || name == "target"));
  };

  cargoHash = "sha256-BuWTx7ohs8cDYRFMY8yklAPjbE5j20ULbrtTaIz2vxg=";

  nativeBuildInputs = [
    pkg-config
    wrapGAppsHook4
  ];

  buildInputs = [
    gdk-pixbuf
    gsettings-desktop-schemas
    gtk4
    sqlite
    webp-pixbuf-loader
  ];

  nativeCheckInputs = [
    curl
    ffmpeg
    git
    gzip
    poppler-utils
    python3
    ripgrep
    vips
    zbar
  ];

  postPatch = ''
    substituteInPlace src/guide_topics.rs src/productivity_smoke.rs \
      --replace '#!/bin/sh' '#!${runtimeShell}'
  '';

  preCheck = ''
    export GDK_PIXBUF_MODULE_FILE=${gdkPixbufLoaders}
  '';

  postInstall = ''
    # Read by the gapps wrapper hook below, so it has to be set here.
    export GDK_PIXBUF_MODULE_FILE=${gdkPixbufLoaders}
    install -Dm644 docs/community-plugins.toml $out/lib/spotlight/community-plugins.toml
    install -Dm644 packaging/spotlight.desktop $out/share/applications/spotlight.desktop
    substituteInPlace $out/share/applications/spotlight.desktop \
      --replace-fail 'Exec=spotlight' "Exec=$out/bin/spotlight"
    install -Dm644 assets/spotlight.svg $out/share/icons/hicolor/scalable/apps/spotlight.svg
    for license in LICENSE assets/UNICODE-LICENSE.txt assets/QRCODEGEN-LICENSE.txt; do
      install -Dm644 "$license" "$out/share/licenses/spotlight/$(basename "$license")"
    done
  '';

  preFixup = ''
    gappsWrapperArgs+=(
      --set GSK_RENDERER cairo
      # The autostart entry has to start the wrapped process.
      --set SPOTLIGHT_EXECUTABLE $out/bin/spotlight
      --prefix PATH : ${
        lib.makeBinPath [
          curl
          python3
          ripgrep
          xdg-utils
        ]
      }
    )
  '';

  meta = {
    description = "A keyboard-first native Linux productivity launcher";
    homepage = "https://github.com/danny19862025/spotlight";
    license = lib.licenses.mit;
    mainProgram = "spotlight";
    platforms = lib.platforms.linux;
  };
}
