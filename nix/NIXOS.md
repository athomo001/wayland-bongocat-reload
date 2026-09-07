# NixOS Installation Guide

> [!WARNING]
> This guide may contain misinformation, please open a PR with the changes if you find something wrong

## Quick Start

### Direct Installation with Flakes

```bash
# Try `wayvpet` without installing
nix run github:saatvik333/wayland-bongocat

# Install to user profile
nix profile install github:saatvik333/wayland-bongocat

# Find your input devices
wayvpet-find-devices
```

### Using the NixOS Module (Recommended)

Remember to rebuild your system!

#### With Flakes (Recommended)

If you use flakes for your NixOS configuration (Which you should):

```nix
# In your `flake.nix`
{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    wayvpet.url = "github:saatvik333/wayland-bongocat";
  };

  outputs = inputs: {
    nixosConfigurations.your-hostname = inputs.nixpkgs.lib.nixosSystem {
      system = "x86_64-linux";
      modules = [
        ./configuration.nix
        inputs.wayvpet.nixosModules.default
        {
          programs.wayvpet = {
            enable = true;
            autostart = true;
          };
        }
      ];
    };
  };
}
```

#### Without Flakes

Download this repository wherever you desire and add this to your NixOS configuration:

```nix
{ config, pkgs, ... }: {
  # Import the module
  imports = [
    # ... your other imports
    /path/to/wayvpet/nix/nixos-module.nix
  ];

  # Enable and configure wayvpet
  programs.wayvpet = {
    enable = true;
    autostart = true;        # Start on login by creating a SystemD service

    # Position
    catXOffset = 120;        # Horizontal
    catYOffset = 0;          # Vertical

    # Size (In pixels)
    catHeight = 40;
    overlayHeight = 60;

    # Animation settings
    idleFrame = 0;           # Default idle frame
    keypressDuration = 150;  # Animation duration (ms)
    fps = 60;                # Frame rate

    # Visual
    overlayOpacity = 0;      # Overlay bar transparency

    # REQUIRED - Wayvpet won't work properly without configuring this first
    # Input devices (Find yours with `wayvpet-find-devices`)
    inputDevices = [
      # Example devices
      "/dev/input/event4"
      "/dev/input/event20"
    ];

    # Debug settings
    enableDebug = false;     # Set to true for troubleshooting
  };
}
```

### Using the Home Manager Module

A home manager module is also provided by the repository flake. It's just like the NixOS module, but for home-manager!

```nix
# In your `flake.nix`
{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    home-manager = {
      url = "github:nix-community/home-manager";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    wayvpet.url = "github:saatvik333/wayland-bongocat";
  };

  outputs = inputs: {
    homeConfigurations.user = inputs.home-manager.lib.homeManagerConfiguration {
      # ...
      modules = [
        inputs.wayvpet.homeModule.default
        # ...
      ];
    };
  };
}
```

## Developing

### Using Flakes (RECOMMENDED)

Just enter the development environment with the `nix develop` command and build the project
with `nix build`. This command has to be executed in the same directory as the `flake.nix`
file.

The build output will be stored in `result/` with the binaries in `result/bin` in the
current directory.

## Configuration

Run `wayvpet-find-devices` to find all input devices.

This will show you:

- Available input devices
- Device details with human-readable names
- Suggested configuration entries
- Testing instructions

### Manual Configuration

Create a `wayvpet.conf` file wherever you desire:

```ini
# Multiple input devices
keyboard_device=/dev/input/event4   # Built-in keyboard
keyboard_device=/dev/input/event20  # External keyboard

# Position and size
cat_x_offset=120
cat_y_offset=0
cat_height=40
overlay_height=60

# Animation settings
idle_frame=0
keypress_duration=150
fps=60

# Visual settings
overlay_opacity=0  # Transparent background
enable_debug=0
```

## SystemD Service Management

Set `programs.wayvpet.autostart = true;` to create a SystemD user service so
it gets automatically started upon login.

```bash
# Check service status
systemctl --user status wayvpet

# Start/stop manually
systemctl --user start wayvpet
systemctl --user stop wayvpet

# View logs
journalctl --user -u wayvpet -f
```

## Troubleshooting

### Permission Issues

If you get permission errors accessing input devices:

1. **Check if you're in the input group:** Run `groups | grep input`
1. **Add yourself to the `input` group:** Add to your configuration `users.users.<your username>.extraGroups = ["input"];`
1. **Log out and log back in after you add yourself to the `input` group**

### Service Issues

If the SystemD service fails to start:

1. **Check logs:** `journalctl --user -u wayvpet -n 50`
1. **Test manually:** `wayvpet --config /nix/store/.../wayvpet.conf`
1. **Enable debug mode:** `programs.wayvpet.enableDebug = true;`

### Input Device Detection

If keyboard input isn't detected:

1. **Find your devices:** `wayvpet-find-devices`

1. **Test device events:** `sudo evtest  # Select your device and type`

1. **Update your configuration:**
   If using the NixOS or home-manager module -

   ```nix
   programs.wayvpet.inputDevices = [
       # Add as many devices as required and replace X with your device number
       "/dev/input/eventX"
       "/dev/input/eventX"
       "/dev/input/eventX"
   ];
   ```

   Standalone (In your `wayvpet.conf` file) -

   ```ini
   # Add as many devices as required and replace `X` with the actual device number
   keyboard_device=/dev/input/eventX
   keyboard_device=/dev/input/eventX
   keyboard_device=/dev/input/eventX
   ```

You can also use the recommended `keyboard_name=your keyboard` config suggestion provided by `wayvpet-find-devices`.

In the NixOS or home-manager module -

```nix
programs.wayvpet.inputDeviceNames = ["your keyboard"]
```

### Wayland Compositor Compatibility

Ensure your compositor supports the layer shell protocol:

- ✅ **Hyprland** - Full support
- ✅ **Sway** - Full support
- ✅ **Wayfire** - Compatible
- ✅ **KDE Wayland** - Compatible
- ❌ **GNOME Wayland** - Support Unknown

## Advanced Usage

### Custom Package

You can override the package in the module:

```nix
programs.wayvpet = {
  enable = true;
  package = pkgs.wayvpet.overrideAttrs (old: {
    # Custom build options
    buildInputs = old.buildInputs ++ [ pkgs.someExtraPackage ];
  });
};
```

### Multiple Configurations

You can run multiple instances using different configurations

```bash
# Instance 1
wayvpet --config ~/.config/wayvpet/work.conf &

# Instance 2
wayvpet --config ~/.config/wayvpet/gaming.conf &
```

### Integration with Window Managers

- **Hyprland:** `exec-once = wayvpet`
- **Sway:** `exec wayvpet`

## Building from Source

### Prerequisites

The flake includes all the necessary dependencies to build the project.
Just enter the development environment by running `nix develop` in the
same directory as the `flake.nix` file. The development environment will
provide further information e.g. commands to build the project.

> **Note:** The generated Wayland protocol bindings are committed to git, so `wayland-scanner` and `wayland-protocols` are only needed in the dev shell for `make protocols` (regenerating protocol bindings from XML). Regular builds (`make` / `nix build`) don't require them.

## Contributing

See the [README](../README.md) for contribution guidelines. The NixOS integration welcomes:

- Additional compositor testing
- Module option improvements
- Documentation enhancements
- Bug reports and fixes

## Support

For NixOS-specific issues:

1. Read through this guide thoroughly
1. Read through `nixos-module.nix`
1. Test with the development shell
1. Open an issue with your NixOS version and configuration
