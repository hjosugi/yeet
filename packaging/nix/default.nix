{ lib, rustPlatform, pkg-config, wrapGAppsHook4, gtk4, gtk4-layer-shell }:

rustPlatform.buildRustPackage rec {
  pname = "yeet";
  version = "0.5.0";
  src = lib.cleanSource ../..;
  cargoLock.lockFile = ../../Cargo.lock;

  nativeBuildInputs = [ pkg-config wrapGAppsHook4 ];
  buildInputs = [ gtk4 gtk4-layer-shell ];

  postInstall = ''
    install -Dm644 packaging/linux/io.github.hjosugi.Yeet.desktop -t $out/share/applications
    install -Dm644 packaging/linux/io.github.hjosugi.Yeet.metainfo.xml -t $out/share/metainfo
    install -Dm644 assets/io.github.hjosugi.Yeet.svg $out/share/icons/hicolor/scalable/apps/io.github.hjosugi.Yeet.svg
    install -Dm644 packaging/linux/yeet.1 -t $out/share/man/man1
  '';

  meta = {
    description = "Native Yoink-style drag-and-drop shelf";
    homepage = "https://github.com/hjosugi/yeet";
    license = lib.licenses.mit;
    mainProgram = "yeet";
    platforms = lib.platforms.linux;
  };
}
