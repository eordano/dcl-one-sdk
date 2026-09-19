# Provisioning the upstream UI Designer

The SDK binary provides the project bridge, data-layer proxy, and authenticated
browser runtime routes. UI Designer additionally needs the genuine upstream
inspector frontend and host. The offline inspector shim does not include them.
The verified combination is inspector **7.46.1**, OXC WASM **0.60.0**, and
mini-rpc **1.0.7**, with Node 24 and npm 11 available to the SDK service.

From this crate, provision a new versioned host directory:

```sh
sdk_inspector_host=/srv/dcl/sdk-inspector/7.46.1
test ! -e "$sdk_inspector_host" && \
DCL_ONE_SDK_INSPECTOR_HOST="$sdk_inspector_host" \
  bash scripts/install-inspector-host.sh 7.46.1 && \
bash scripts/install-ui-designer-runtime.sh \
  "$sdk_inspector_host/node_modules/@dcl/inspector"
```

The host installer recreates its target directory, so use a new path when
upgrading an installation. It supplies missing upstream runtime dependencies,
resolves nested asset-pack packages, and defines the production parser flag
while bundling the Node entrypoint. The second installer verifies that the
frontend contains UI Designer and places the exact parser/RPC runtime and WASM
beside it. Keep the host directory and its dependencies readable by the SDK
service; the browser must never receive a filesystem path or credentials.

Configure the scene's existing SDK process:

```sh
DCL_ONE_INSPECTOR_DIR=/srv/dcl/sdk-inspector/7.46.1/node_modules/@dcl/inspector \
DCL_ONE_SDK_ALLOWED_ORIGINS=https://catalyst.example.com \
  dcl-one-sdk start --dir /path/to/scene --data-layer
```

`DCL_ONE_INSPECTOR_DIR` explicitly overrides the scene's installed inspector,
including an offline shim. Its package dependencies are preferred, with normal
scene dependency resolution as a fallback. A broken explicit inspector is
reported rather than silently replaced by the shim. This does not require
changing the scene's package manifest or replacing its node_modules.

Open the SDK project from Creator Hub. Its `/api/project` descriptor advertises
`links.uiDesigner` and `links.uiDesignerRuntime` only when the data layer, actual
frontend bundle, and pinned runtime are present. Creator Hub retains its ribbon
and hosts the upstream designer with the shared CodeParser/IframeStorage bridge.
The inspector iframe has compatible COEP/CORP headers; runtime requests retain
the SDK's local-peer and trusted-origin checks. Forwarded URL prefixes are
included in descriptor links. Keep the SDK watcher enabled so TSX saves rebuild.

Deployment of the binary alone leaves these links unavailable on unprovisioned
self-hosted SDK installations. Services serving only the editor/system scene or
an empty preview realm do not need the inspector package unless they also expose
an authoring project with `--data-layer`. Provision each authoring SDK process
that should offer UI Designer.

Validation should cover the descriptor, a successful runtime/WASM request from
the configured Creator Hub origin, and a designer property edit saved to the
project's TSX and rebuilt into `bin/scene.js`. The standing browser verifier is
`tools/screen-tour/verify-ui-designer.mjs`; it also checks external edits and
preservation of Unicode and opaque expressions. No model provider is involved.
