{
  lib,
  config,
  pkgs,
  ...
}: let
  cfg = config.programs.wayland-wayvpet;
in {
  imports = [./common.nix];
  config = lib.mkIf cfg.enable (let
    configFile = config._wayvpet.configFile;
  in {
    home.packages = [
      cfg.package

      # Helper scripts
      # For starting `wayland-wayvpet` using the config file defined with Nix
      (pkgs.writeScriptBin "wayvpet-exec" ''
        #!${pkgs.bash}/bin/bash
        exec ${cfg.package}/bin/wayvpet --config ${configFile}
      '')
    ];

    # SystemD service
    systemd.user.services.wayland-wayvpet = lib.mkIf cfg.autostart {
      Unit = {
        Description = "Wayland wayvpet Overlay";
        PartOf = ["graphical-session.target"];
        After = ["graphical-session.target"];
      };

      Install = {
        WantedBy = ["graphical-session.target"];
      };

      Service = {
        Type = "exec";
        ExecStart = "${cfg.package}/bin/wayvpet --config ${configFile}";
        Restart = "on-failure";
        RestartSec = "5s";
      };
    };
  });
}
