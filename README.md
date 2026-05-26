# Tautomorphism

Compact selected-text translator for CachyOS / Hyprland / Wayland.

## Binaries

- `tautomorphism`: translate selected text.
- `tautomorphism-config`: settings.

## Dependencies

```bash
sudo pacman -S --needed rust cargo gtk4 glib2 pkgconf wl-clipboard hyprland
```

## Build

Run Command:
```bash
./scripts/build-release.sh
```

The project also enables native CPU code generation by default:
```toml
[build]
rustflags = ["-C", "target-cpu=native"]
```
See `Cargo.toml`.

## Install

Run command:
```bash
sudo install -Dm755 target/release/tautomorphism /usr/local/bin/tautomorphism
sudo install -Dm755 target/release/tautomorphism-config /usr/local/bin/tautomorphism-config
```

## Hyprland

For instance, add these into the hyprland keybinds file:
```ini
bind = SUPER, T, exec, tautomorphism
bind = SUPER SHIFT, T, exec, tautomorphism-config
```

## Config

The config file is saved on `~/.config/tautomorphism/config.json`

Open settings GUI by running command:
```bash
tautomorphism-config
```

## Translation engines

- Google Free: default, no key.
- Google Cloud: optional key.
- Baidu: optional appid/secret.

Common Google language codes: `zh-CN`, `zh-TW`, `en`, `ja`, `ko`, `fr`.

Common Baidu language codes: `zh`, `en`, `jp`, `kor`.


## Other

Recommended Hyprland rule if your compositor still focuses the popup:
```ini
windowrule = match:class ^(dev.orderization.Tautomorphism)$, no_initial_focus on
windowrule = match:class ^(dev.orderization.Tautomorphism)$, no_focus on
```
