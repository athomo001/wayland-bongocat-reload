# NixOS module for wayland-wayvpet
{
  config,
  lib,
  pkgs,
  ...
}:
with lib; let
  cfg = config.programs.wayland-wayvpet;
in {
  imports = [./common.nix];
  config = lib.mkIf cfg.enable (let
    configFile = config._wayvpet.configFile;
  in {
    environment.systemPackages = [
      cfg.package

      # Helper scripts
      # For starting `wayland-wayvpet` using the config file defined with Nix
      (pkgs.writeScriptBin "wayvpet-exec" ''
        #!${pkgs.bash}/bin/bash
        exec ${cfg.package}/bin/wayvpet --config ${configFile}
      '')
    ];

    # SystemD service
    systemd.user.services.wayland-wayvpet = mkIf cfg.autostart {
      enable = true;
      description = "Wayland wayvpet Overlay";
      wantedBy = ["graphical-session.target"];
      partOf = ["graphical-session.target"];
      after = ["graphical-session.target"];
      serviceConfig = {
        Type = "exec";
        ExecStart = "${cfg.package}/bin/wayvpet --config ${configFile}";
        Restart = "on-failure";
        RestartSec = "5s";
      };
    };
  });
}
