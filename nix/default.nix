{
  lib,
  stdenv,
  pkg-config,
  wayland,
}:
stdenv.mkDerivation (finalAttrs: {
  pname = "wayvpet";
  version = "2.0.2";
  src = ../.;

  # Build toolchain and dependencies
  # Protocol bindings are pre-generated and committed to git, so
  # wayland-scanner and wayland-protocols are only needed for `make protocols`.
  strictDeps = true;
  nativeBuildInputs = [pkg-config];
  buildInputs = [
    wayland
  ];

  makeFlags = ["release"];
  installPhase = ''
    runHook preInstall

    # Install binaries
    install -Dm755 build/wayvpet $out/bin/${finalAttrs.meta.mainProgram}
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
