# Running this crate's tests

```
cargo test -p dcl-one-sdk
```

is the contract: **it passes on a machine that has only this checkout, a
toolchain and `node`**, and it never reports a test as `ok` that did not assert
anything. node is the one dependency deliberately *not* hidden behind
`#[ignore]` — see below — so on a machine without it the eight `golden` tests
fail rather than skip.

Everything else that needs a resource this repo cannot provide is `#[ignore]`d
with a reason, so the harness prints the reason instead of a green tick:

```
test two_member_workspace_builds_serves_and_reloads_per_member ... ignored, needs DCL_ONE_SDK_TEST_NODE_MODULES ...
```

`ignored` in the tally is the honest count of what was not run. A bare
`#[ignore]` with no reason is rejected by `tests/testgate_contract.rs`, and so
is a gating variable that is not listed on this page.

## Turning the gated tests on

```
DCL_ONE_SDK_TEST_NODE_MODULES=/path/to/scene/node_modules \
DCL_ONE_SDK_TEST_SCENE=/path/to/scene \
  cargo test -p dcl-one-sdk -- --include-ignored
```

Under `--include-ignored` the gate is *armed*: a variable that is missing makes
the test **fail** naming the variable, rather than passing silently. That is
`catalyrst-testgate`, and it is the point of the crate — asking for the
heavyweight suite and getting a green run that skipped it is the failure mode it
exists to stop.

| variable | what it unlocks | what to point it at |
| --- | --- | --- |
| `DCL_ONE_SDK_TEST_NODE_MODULES` | `data_layer_rpc`, `initial_build_recovery`, `workspace_e2e` — everything that drives a real `node` client against the preview server — plus `data_layer_ui`, which exists only in the upstream source tree | the `node_modules` directory of a scene with `@dcl/sdk`, `@dcl/rpc`, `@dcl/inspector` and `ws` installed. It is symlinked into the fixture, so it must be on the same filesystem as `$TMPDIR`. |
| `DCL_ONE_SDK_TEST_SCENE` | `init_scaffold`'s provisioned-build test, and `error_contract`'s `t2`/`t6` compiler-diagnostic tests | the root of a scene checkout that has already had its dependencies installed (`<scene>/node_modules/@dcl/sdk` must exist) |
| `DCL1_TUNNEL_PUBLIC_URL` | `tunnel_live` — drives a tunnel origin that is actually deployed | the public base URL of a running preview tunnel, e.g. `https://t.example.com/abc123` |
| `DCL1_TUNNEL_LOCAL_URL` | optional, same test | the local preview the tunnel fronts, when it is not the default |
| `DCL1_TUNNEL_TOUCH_FILE` | optional, same test | a file inside the served scene, touched to prove live reload crosses the tunnel |

`data_layer_ui` and its driver `scripts/creator-hub-ui-drive.sh` are **not in
the published tree**: `dcl-one-sdk-standalone-assemble.sh` excludes `scripts/`
(bar `pin-abgen.sh` and `golden-runtime.mjs`) and `tests/data_layer_ui.rs`, as
dev harnesses that reach for private tooling. Both live in the upstream source
checkout only, where the test additionally needs a chromium on the machine.

**`node` is not on this list on purpose.** `golden`'s runtime tier needs it and
gates on it at runtime rather than with `#[ignore]`, because the crate's own
`build` cannot type-check without node either: a machine missing it cannot use
this tool at all, so a red test is the right answer there, not a quiet skip. It
fails naming `node` and pointing at the opt-out below.

## The escape hatch, and what it costs you

| variable | effect |
| --- | --- |
| `ALLOW_SKIPPED_INTEGRATION=1` | a missing dependency returns instead of failing. The test then reports `ok` having asserted nothing, so `catalyrst-testgate` prints a `SKIPPED <test>: ...` line straight to fd 2 — outside libtest's output capture, which discards the stderr of passing tests. Use it in a CI matrix leg that genuinely cannot host the dependency, and read the SKIPPED lines. |
| `CATALYRST_TESTGATE_SKIPLOG` | a file every skip is appended to, one `test<TAB>requirement<TAB>detail` record per line. Read this instead of the pass tally when the hatch is open. |

## Other knobs

| variable | effect |
| --- | --- |
| `UPDATE_GOLDEN=1` | rewrite `testdata/golden/*.golden` from the current output instead of comparing: `UPDATE_GOLDEN=1 cargo test -p dcl-one-sdk --test golden`. A missing golden still fails without it. (In the upstream source tree the same thing is one command, `scripts/update-goldens.sh` at the workspace root; that wrapper is not part of the published crate.) |
| `DCL_ONE_SCHEMA_FUZZ_SEED` | replay one `schema_parity` fuzz seed; the run prints the seed it chose. |

## Timing

Every test that spawns the CLI goes through `error_contract.rs`'s `run()`, which
kills the child and fails naming the arguments once it passes
`CHILD_TIMEOUT`. Nothing in this suite is allowed to wait on a default
product timeout — `deploy` without a key waits ten minutes for a browser
signature (`DCL_ONE_SDK_LINKER_TIMEOUT_SECS`, default 600), and a test that
reaches that path must fail in seconds, not turn CI into a twelve-minute outage.
