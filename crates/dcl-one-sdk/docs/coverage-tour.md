# dcl-one-sdk coverage tour

LLVM source-based line coverage of the crate's own code (`crates/dcl-one-sdk/src`),
measured by running the existing test suites plus a whole-CLI "tour" that drives
every user-facing command and probes the running servers.

## Result

| | lines | covered | line % |
|---|---|---|---|
| baseline (existing suites only) | 13285 | 9610 | 72.34% |
| final (baseline + tour) | 13285 | 11595 | 87.28% |

Scope: `crates/dcl-one-sdk/src` only (`--ignore-filename-regex` excludes deps and
workspace siblings). The percentage counts in-file `#[cfg(test)]` modules, which
are always fully hit when their tests run.

## How to reproduce

Instrument (devshell; the two `llvm-args` enable `LLVM_PROFILE_FILE=%c`
continuous mode so servers killed mid-run still write a mergeable profile):

```
export CARGO_TARGET_DIR=target/cov
export RUSTFLAGS="-C instrument-coverage \
  -C llvm-args=--runtime-counter-relocation \
  -C llvm-args=--instrprof-atomic-counter-update-all"
cargo test -p dcl-one-sdk --no-run
cargo build -p catalyrst-preview-tunnel        # tour case 94 runs a real tunnel
```

Baseline — the env-gated suites must run, not skip:

```
export DCL_ONE_SDK_TEST_NODE_MODULES=<blob node_modules>   # 7.26.0 install tree
export DCL_ONE_SDK_TEST_SCENE=<scene with @dcl/sdk-commands installed>
export ALLOW_SKIPPED_INTEGRATION=1
export LLVM_PROFILE_FILE="<dir>/base-%p-%m%c.profraw"
cargo test -p dcl-one-sdk
```

Tour:

```
scripts/coverage-tour.sh target/cov/debug/dcl-one-sdk <scratch-workdir>
# optional 3rd arg: a grep -E filter over "NN name" to run a subset
# TOUR_TRACE=1 turns on set -x per case
# TOUR_NPM_SCENE / TOUR_TUNNEL_BIN override the signing/tunnel deps
```

Merge and report with the toolchain-matched LLVM (the devshell rustc 1.95.0
reports LLVM 21.1.8 — the `rust-toolchain.toml` 1.97.0 pin is the export CI's,
not the devshell's; nixpkgs `llvmPackages_21.libllvm` provides a byte-compatible
`llvm-profdata` / `llvm-cov`):

```
OBJ="target/cov/debug/dcl-one-sdk \
  $(for t in target/cov/debug/deps/<each test bin>; do printf ' -object %s' $t; done) \
  -object target/cov/debug/catalyrst-preview-tunnel"
llvm-profdata merge -sparse -o final.profdata <all>/*.profraw
llvm-cov report --instr-profile=final.profdata $OBJ crates/dcl-one-sdk/src
```

## Tooling caveats

- The devshell has no `cargo-llvm-cov`/`llvm-profdata`/`llvm-cov`; the manual
  path above was chosen (over fetchable `nixpkgs#cargo-llvm-cov`) so server
  commands can be killed while keeping their profiles.
- Plain `-C instrument-coverage` does **not** support continuous (`%c`) mode; a
  `%c` profile aborts with *"Neither __llvm_profile_counter_bias nor
  __llvm_profile_bitmap_bias is defined"*. The two extra `llvm-args` turn it on.

## Case inventory

122 cases pass, 0 fail, 1 documented skip. Each is a numbered function in
`scripts/coverage-tour.sh`; the driver's `tcase` table is the authoritative list.
The harness, stub servers, node sidecars, and fixture writers live in the
sourced `scripts/coverage-tour-lib.sh` beside it. Cases have a forward
dependency order (e.g. `07 init_scene` builds the shared scene the later cases
reuse), so run the whole script, not arbitrary subsets.

- CLI surface: 01 version, 02 help_top, 03 help_subcommands, 04 unknown_flag,
  05 unknown_subcommand, 06 no_args, 92 verbose_error_chain,
  93 rust_log_implies_verbose
- init: 07 scene, 08 refuses_non_empty, 09 yes_non_empty, 10 smart_wearable,
  11 node_modules_only, 12 non_tty_defaults_scene, 13 tty_prompt_wearable,
  14 tty_prompt_bad_answer, 15 target_is_file
- get-context-files: 16 offline_in_project, 17 outside_project,
  18 download_from_stub, 19 unreachable_api, 20 partial_failure
- build: 21 dev, 22 production, 23 skip_type_check, 24 custom_entry_point,
  25 composite_scene, 26 ignore_composite, 27 workspace, 28 syntax_error,
  29 type_error, 30 broken_composite, 31 oversize_composite (>16 MiB refused,
  not fatal), 32 watch_edit_rebuilds (asserts the `rebuilt` marker after the
  edit; SIGINT is cleanup, not a claim), 33 watch_workspace_rebuilds (same, on
  a member edit), 34 missing_tsconfig, 35 workspace_broken_member,
  99 crdt_via_sdk_commands, 115 opera_composite, 123 no_node_for_typecheck,
  124 watch_typecheck_rebuilds (asserts the `rebuilt` marker with type
  checking on), 125 smart_chunk_missing, 98 vendor_chunks (hidden
  `vendor-chunks`), 126 golden_runtime_compare (builds a golden fixture with
  this binary and diffs the runtime harness output against the checked-in
  golden — compare only, it never rewrites; see docs/golden-snapshots.md)
- start: 36 probe_suite (/about, /, /scenes, /content/contents b64 + 304 + HEAD +
  ignored-file 404 + outside-root 403, entities active/scene, mobile-preview,
  lambdas, inspector 404/redirect/traversal, `/` and `/mini-comms` ws upgrade),
  37 offline_comms, 38 data_layer, 39 mobile, 40 workspace, 41 watch_reload
  (ts + .glb UMT_CHANGE + .glb delete UMT_REMOVE + composite regen),
  42 skip_build, 43 flag_compat_deeplink, 44 abgen_hint, 45 default_port,
  49 port_in_use, 100 backfill_proxy, 101 inspector_ui_injected,
  102 failed_build_recovers, 103 config_error_fatal, 112 workspace_watch,
  113 data_layer_no_node, 114 data_layer_broken, 116 abgen_fake_sidecar,
  119 watch_registry_flip (asserts the live `rebuilt` marker after the script
  component lands), 120 landing_rich_scene, 121 privileged_port,
  122 data_layer_allowed_origins
- tunnel: 46 help, 47 invalid_url, 48 unreachable_warns, 94 real_service (against
  a locally-run `catalyrst-preview-tunnel`), 118 token_file_missing
- deploy: 51 dry_run_deterministic, 52 key_env_to_content, 53 sign_key_via_target,
  54 key_needs_target, 55 world_needs_server, 56 conflicting_targets,
  57 linker_flow_timeout, 58 world_key_yes_fallback, 59 world_prompt_non_tty,
  60 world_denied, 61 default_target_catalyst, 62 default_target_content,
  63 rotation_browser_flow, 64 unreachable_server, 65 at_workspace_root,
  95 linker_completed (browser page signed with a throwaway key via node),
  104 multi_scene_world, 106 consent_refused_ci (SKIP), 107 world_prompt_pty,
  108 world_parcel_allowlist, 109 server_rejects, 110 world_linker_del_sig,
  111 linker_no_xdg_open
- unpublish: 66 ok, 67 404_hint, 68 bad_parcel, 69 no_target, 70 unreachable,
  71 no_key
- world: 72 settings_get, 73 settings_get_unreachable, 74 settings_get_refused,
  75 settings_set_title, 76 settings_set_all_fields, 77 settings_set_nothing,
  78 settings_set_denied, 79 permissions_list, 80 permissions_grant,
  81 permissions_revoke, 82 permissions_bad_perm, 83 permissions_bad_address,
  84 linker_flow_timeout, 85 set_unreachable, 96 linker_completed,
  97 linker_denied_alive
- pack: 86 valid_wearable, 87 not_a_wearable, 88 schema_violation,
  89 malformed_wearable_json, 90 oversize_warns, 91 missing_referenced_file,
  105 repack_replaces_zip

### Skipped case

- **106 deploy_consent_refused_ci** — the public-network consent gate
  (`deploy/net.rs:213-231`) fires only when the resolved target is on the real
  upstream catalyst rotation; an env-supplied rotation is treated as an explicit
  target and bypasses it. Not reachable offline. The same code is driven from the
  TTY side by 107 (prompt) and the non-interactive refusal below it is covered.

## Per-file line coverage (baseline → final)

| file | lines | base % | final covered | final % |
|---|---|---|---|---|
| start/mod.rs | 920 | 82.2 | 874 | 95.0 |
| tunnel.rs | 734 | 31.3 | 533 | 72.6 |
| deploy/net.rs | 699 | 30.0 | 577 | 82.5 |
| joinblock.rs | 676 | 99.4 | 673 | 99.6 |
| world.rs | 543 | 65.2 | 499 | 91.9 |
| composite_norm.rs | 541 | 82.4 | 446 | 82.4 |
| deploy/mod.rs | 527 | 86.7 | 467 | 88.6 |
| crdt_gen.rs | 475 | 65.9 | 315 | 66.3 |
| start/landing.rs | 457 | 79.7 | 438 | 95.8 |
| linker.rs | 450 | 70.7 | 354 | 78.7 |
| entrypoint.rs | 432 | 87.0 | 380 | 88.0 |
| data_layer.rs | 427 | 71.2 | 371 | 86.9 |
| start/http.rs | 425 | 70.3 | 392 | 92.2 |
| init.rs | 397 | 84.4 | 354 | 89.2 |
| watch.rs | 383 | 74.4 | 307 | 80.2 |
| asset_bundles.rs | 379 | 74.1 | 351 | 92.6 |
| ux.rs | 368 | 86.1 | 325 | 88.3 |
| world_linker.rs | 362 | 69.1 | 308 | 85.1 |
| jsjson.rs | 351 | 87.8 | 312 | 88.9 |
| pack.rs | 347 | 84.2 | 303 | 87.3 |
| main.rs | 326 | 53.1 | 315 | 96.6 |
| scene.rs | 317 | 98.1 | 311 | 98.1 |
| context_files.rs | 279 | 86.0 | 240 | 86.0 |
| build.rs | 241 | 67.2 | 192 | 79.7 |
| comms.rs | 233 | 79.8 | 187 | 80.3 |
| split.rs | 222 | 93.2 | 210 | 94.6 |
| start/proxy.rs | 207 | 0.0 | 175 | 84.5 |
| deploy/run.rs | 197 | 67.5 | 165 | 83.8 |
| start/editor.rs | 196 | 51.0 | 158 | 80.6 |
| prebuilt.rs | 186 | 52.7 | 162 | 87.1 |
| workspace.rs | 179 | 98.3 | 176 | 98.3 |
| start/content_cache.rs | 154 | 95.5 | 147 | 95.5 |
| skills.rs | 143 | 91.6 | 131 | 91.6 |
| live_reload.rs | 132 | 94.7 | 125 | 94.7 |
| rolldown_backend.rs | 105 | 82.9 | 89 | 84.8 |
| deploy/unpublish.rs | 96 | 27.1 | 92 | 95.8 |
| netinfo.rs | 94 | 98.9 | 93 | 98.9 |
| abgen_embed.rs | 48 | 25.0 | 12 | 25.0 |
| esbuild.rs | 31 | 100.0 | 31 | 100.0 |
| lib.rs | 6 | 83.3 | 5 | 83.3 |
| **TOTAL** | **13285** | **72.3** | **11595** | **87.3** |

## Uncovered-region inventory

1690 lines remain uncovered. Classified below; `file:line` ranges are the
largest contiguous blocks per file (`llvm-cov show` / lcov `DA:` zeros).

### (a) Drivable — not yet driven

- **crdt_gen.rs** 437-453, 307-315, 288-293, 147-162 (~125 lines total, 66%):
  the CRDT binary encoder's map/packed-array/enum/bytes branches. Reachable by
  building composites that exercise every component field kind; the tour's
  composites hit only Transform/MeshRenderer. A composite fixture covering each
  `FieldKind` (map, repeated, enum-by-name, base64 bytes) would close most.
  Numbers predate the ISchema encoder below and are stale for this file.
- **schema_crdt.rs** (new since this table): the second of main.crdt's two
  encoders. crdt_gen serializes `core::*` as protobuf against the vendored
  @dcl/protocol descriptors; everything else — `inspector::*`,
  `core-schema::*`, `asset-packs::*` and user-named components — is serialized
  here against the `jsonSchema` the composite itself carries, which is the only
  way a user-defined component can be encoded at all. Driven by the golden
  fixtures in `testdata/{opera,gather,gather2,museum}-main.{composite,crdt}`
  (each `.crdt` regenerated through the node data-layer when it was committed,
  @dcl/ecs 7.26.0) and by `tests/schema_parity.rs`, a seeded differential fuzz
  against @dcl/ecs itself. `scripts/crdt-diff.py` turns any parity failure into
  a component + entity + schema field path, and `scripts/ischema-oracle.py` is
  an independent Python reimplementation kept as a third opinion. In production,
  `DCL_ONE_CRDT_VERIFY=1` runs the node data-layer alongside the native path and
  logs a decoded diff on any divergence.
- **composite_norm.rs** 300-310, 213-221, 262-270 (~90 lines, 82%): normalizer
  branches for component shapes absent from the tour composites. Same fix —
  broader composite fixtures (the `docs/composite-tojson-edge-cases.json` set).
- **jsjson.rs** scattered (~39): `JSON.stringify` number/string formatting edge
  cases (exponent forms, control-char escapes). Drivable with a targeted value.
- **comms.rs** 154-173 (~20): the same-address second-login kick across rooms;
  needs a third comms client re-using an address already in another room.
- **deploy/net.rs** 370-386 (world-wide grant summary parse) — reachable with a
  stub returning a `summary` with `world_wide:true`.
- **init.rs** 313-321, 134-146; **pack.rs** 100-105, 87-91 (empty-fileset /
  case-collision guards) — reachable with crafted `.dclignore` fixtures.

### (b) Needs the external world (real services / browser / TTY)

- **tunnel.rs** 497-513, 616-681, 530-553 (~195 lines, 72%): channel
  read-error, local-preview-unreachable, and close-frame relay paths inside the
  live trunk multiplexer. Case 94 drives the happy path against a local tunnel;
  the failure/close paths need induced socket errors or a real disconnect.
- **linker.rs** 630-663: the `#[cfg(test)]` `sign_flow_completes_against_local_worlds`
  smoke test, gated on `DCL_ONE_SDK_LINKER_SMOKE_KEY` + a live worlds server.
- **deploy/net.rs** 215-237 partial, 269-274: the interactive `Continue?`/public
  consent branches that only run on a real TTY against the real rotation
  (case 107 covers the TTY prompt via `script`; the rotation-side is 106/skip).
- **context_files.rs** 139-147, 232-235: real GitHub contents-API listing/retry
  timing paths beyond what the local stub reproduces.
- **start/editor.rs** 21-37, 69-73: mobile-preview QR failure + data-layer
  restart/backoff paths that need an induced QR/driver fault.

### (c) Defensive / OS-error paths

- **watch.rs** 30-62, 432-440: inotify-limit `UserError` construction and the
  sdk-registry rebuild failure branch (fires only on a filesystem/build error).
- **start/mod.rs** 518-527, 603-613: `retry_initial_build` inner failure loop and
  the `PermissionDenied` bind arm (case 121 hits the privileged-port arm on
  non-root; the generic `binding {addr}` fallback stays defensive).
- **ux.rs** 234-261: the slow-operation spinner thread body, which only paints
  after `SLOW_AFTER` seconds of a blocking op — timing-only, no assertion hook.
- **deploy/mod.rs** 320-340, **build.rs** 195-211: `no_parcels()` / smart-chunk /
  metadata-shape error constructors reached only on malformed input the earlier
  validation already rejects.
- Scattered `.unwrap_or_else(PoisonError::into_inner)` and `map_err` arms across
  world.rs, world_linker.rs, data_layer.rs, run.rs — mutex-poison and
  I/O-failure guards.

### (d) Platform / build-config specific

- **abgen_embed.rs**: no longer build-config specific. Every build embeds abgen
  (see `abgen-release.lock`), so `FILES` is never empty and `extract_into` runs
  in dev too; `the_binary_carries_an_abgen` and
  `extraction_yields_a_runnable_binary_and_is_idempotent` cover it directly.
  Case 116 still covers the external-`ABGEN_BIN` sidecar.
- **netinfo.rs** / **joinblock.rs** residuals: interface-class branches
  (overlay-VPN, virtual-bridge, NAT-VM guest) that depend on the host's actual
  network interfaces.
- **linker.rs**/**world_linker.rs** `spawn_browser` non-Linux arms (`open` /
  `explorer`) — `cfg`-selected for macOS/Windows.
