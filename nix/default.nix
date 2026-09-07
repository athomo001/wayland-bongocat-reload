{
  lib,
  stdenv,
  cargo,
  rustc,
  pkg-config,
  wayland,
  libxkbcommon,
}:
stdenv.mkDerivation (finalAttrs: {
  pname = "wayvpet";
  version = "3.0.0-dev";
  src = ../.;

  # El workspace se compila con Cargo; declarar ambos ejecutables es necesario
  # porque `make release` delega directamente en `cargo build`.
  strictDeps = true;
  nativeBuildInputs = [
    cargo
    rustc
    pkg-config
  ];
  buildInputs = [
    wayland
    libxkbcommon
  ];

  makeFlags = ["release"];
  installPhase = ''
    runHook preInstall

    # Install binaries
    install -Dm755 target/release/wayvpet $out/bin/${finalAttrs.meta.mainProgram}
    install -Dm755 scripts/find_input_devices.sh $out/bin/wayvpet-find-devices
    
    # Install man page
    install -Dm644 man/wayvpet.1 $out/share/man/man1/wayvpet.1
    install -Dm644 wayvpet.conf.example $out/share/wayvpet/wayvpet.conf.example

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
