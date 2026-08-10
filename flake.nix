{
  description = "GTK Office Suite — Letters, Tables and Decks (GNOME-native office suite in Rust)";

  # NOTE: flake.lock is not committed yet — it must be generated on a
  # machine with Nix installed (`nix flake lock`), then committed to pin
  # the nixpkgs input. See the README "Nix" section for usage.
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  };

  outputs =
    { self, nixpkgs }:
    let
      supportedSystems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forAllSystems = nixpkgs.lib.genAttrs supportedSystems;
    in
    {
      packages = forAllSystems (
        system:
        let
          pkgs = nixpkgs.legacyPackages.${system};

          gtkOfficeSuite = pkgs.rustPlatform.buildRustPackage {
            pname = "gtk-office-suite";
            version = "0.1.0";
            src = self;

            # The workspace pins IronCalc and rdocx to git revisions; the
            # lockfile records those revisions, and allowBuiltinFetchGit
            # fetches them without maintaining per-crate output hashes.
            cargoLock = {
              lockFile = ./Cargo.lock;
              allowBuiltinFetchGit = true;
            };

            nativeBuildInputs = [
              pkgs.glib # glib-compile-schemas
              pkgs.pkg-config
              pkgs.wrapGAppsHook4
            ];

            buildInputs = [
              pkgs.cairo
              pkgs.glib
              pkgs.gsettings-desktop-schemas
              pkgs.gtk4
              pkgs.libadwaita
              pkgs.pango
            ];

            # Window-level tests require a display and a running session;
            # CI (cargo nextest) covers testing.
            doCheck = false;

            installPhase = ''
              runHook preInstall

              for app in letters tables decks; do
                install -Dm755 "target/release/$app" -t "$out/bin"
              done

              # `nix run . -- <app>` dispatcher; defaults to Letters.
              cat > "$out/bin/gtk-office-suite" <<'EOF'
              #!/usr/bin/env bash
              set -euo pipefail
              app=''${1:-letters}
              shift || true
              exec "$(dirname "$0")/$app" "$@"
              EOF
              chmod +x "$out/bin/gtk-office-suite"

              # GSettings schemas are required at startup (gio::Settings::new).
              install -Dm644 flatpak/org.tunaos.*.gschema.xml -t "$out/share/glib-2.0/schemas"
              glib-compile-schemas "$out/share/glib-2.0/schemas"

              # Desktop integration, mirroring the Flatpak manifests.
              install -Dm644 flatpak/org.tunaos.*.desktop -t "$out/share/applications"
              install -Dm644 flatpak/org.tunaos.*.metainfo.xml -t "$out/share/metainfo"
              install -Dm644 flatpak/icons/org.tunaos.*.svg -t "$out/share/icons/hicolor/scalable/apps"

              runHook postInstall
            '';

            meta = {
              description = "GNOME-native office suite in Rust — Letters (word processor), Tables (spreadsheet), Decks (presentations)";
              homepage = "https://github.com/tuna-os/gtk-office-suite";
              license = pkgs.lib.licenses.gpl3Plus;
              platforms = pkgs.lib.platforms.linux;
              mainProgram = "gtk-office-suite";
            };
          };
        in
        {
          default = gtkOfficeSuite;
          gtk-office-suite = gtkOfficeSuite;
          letters = gtkOfficeSuite // { meta = gtkOfficeSuite.meta // { mainProgram = "letters"; }; };
          tables = gtkOfficeSuite // { meta = gtkOfficeSuite.meta // { mainProgram = "tables"; }; };
          decks = gtkOfficeSuite // { meta = gtkOfficeSuite.meta // { mainProgram = "decks"; }; };
        }
      );

      devShells = forAllSystems (
        system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
        in
        {
          default = pkgs.mkShell {
            nativeBuildInputs = [
              pkgs.cargo
              pkgs.pkg-config
              pkgs.rustc
            ];
            buildInputs = [
              pkgs.cairo
              pkgs.glib
              pkgs.gsettings-desktop-schemas
              pkgs.gtk4
              pkgs.libadwaita
              pkgs.pango
            ];
            # Same schema dir the justfile smoke tests use, so
            # `cargo run -p letters` works out of the box.
            env.GSETTINGS_SCHEMA_DIR = "${toString ./flatpak}";
          };
        }
      );
    };
}
