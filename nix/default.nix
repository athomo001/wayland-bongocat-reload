{
  lib,
  rustPlatform,
  pkg-config,
  wayland,
  libxkbcommon,
}:
rustPlatform.buildRustPackage (finalAttrs: {
  pname = "wayvpet";
  version = "3.0.0-dev";
  src = ../.;

  # Nix descarga y fija las crates a partir del lockfile antes de entrar en el
  # sandbox de compilacion; asi Cargo no necesita acceder a crates.io durante
  # el build.
  cargoLock.lockFile = ../Cargo.lock;
  strictDeps = true;
  nativeBuildInputs = [pkg-config];
  buildInputs = [
    wayland
    libxkbcommon
  ];

  cargoBuildFlags = ["--workspace"];
  installPhase = ''
    runHook preInstall

    # Instalar los tres binarios publicos del workspace.
    install -Dm755 target/release/wayvpet $out/bin/${finalAttrs.meta.mainProgram}
    install -Dm755 target/release/wayvpetctl $out/bin/wayvpetctl
    install -Dm755 target/release/wayvpet-config $out/bin/wayvpet-config
    install -Dm755 scripts/find_input_devices.sh $out/bin/wayvpet-find-devices

    install -Dm644 man/wayvpet.1 $out/share/man/man1/wayvpet.1
    install -Dm644 wayvpet.conf.example $out/share/wayvpet/wayvpet.conf.example
    install -Dm644 packaging/wayvpet.desktop $out/share/applications/wayvpet.desktop
    install -Dm644 packaging/wayvpet-config.desktop $out/share/applications/wayvpet-config.desktop
    install -Dm644 assets/tray/wayvpet-icon.png \
      $out/share/icons/hicolor/64x64/apps/wayvpet.png

    for preset in presets/*.conf; do
      install -Dm644 "$preset" "$out/share/wayvpet/$preset"
    done
    find themes -type f -exec install -Dm644 {} "$out/share/wayvpet/{}" \;

    runHook postInstall
  '';

  # Package information
  meta = {
    description = "Delightful Wayland overlay that displays an animated bongo cat reacting to your keyboard input!";
    homepage = "https://github.com/saatvik333/wayvpet";
    license = lib.licenses.mit;
    maintainers = with lib.maintainers; [voxi0];
    mainProgram = "wayvpet";
    platforms = lib.platforms.linux;
  };
})
