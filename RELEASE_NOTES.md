# dcl-one-sdk v0.25.0

Preview voice infrastructure is now included: `start` launches an embedded LiveKit v1.13.7 server on supported Linux and Windows targets and mints signed realm and scene room tokens. Each preview uses its own random credentials and free signalling/media ports.

- `--no-livekit` retains the built-in ws-room.
- `--livekit-url` with credentials selects an external SFU.
- Tunnel previews retain ws-room unless an external SFU is configured.
- macOS Cargo/release builds require `livekit-server` on PATH; Nix builds embed nixpkgs LiveKit.
- Embedded SFU token validation, tampered-token rejection, SIGTERM and Linux SIGKILL cleanup were verified locally.

Explorer audio and LAN media connectivity remain unverified. Authoritative multiplayer hosts still use mini-comms; use `--no-livekit` for those scenes.

Binaries also include the abgen asset-bundle server. Builds use Rust 1.97.0 and the committed Cargo.lock.
