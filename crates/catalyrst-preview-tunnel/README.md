# catalyrst-preview-tunnel

Self-hosted internet reach for `dcl-one-sdk start`. The creator's machine dials OUT one persistent WebSocket (the trunk) - no inbound ports, no DNS, no certs on the creator side. The service allocates a public path-based route `/t/<id>` and multiplexes every inbound HTTP request AND WebSocket (mini-comms, scene-update live reload) back over that trunk.

```
[explorer clients] --https/wss--> nginx --> catalyrst-preview-tunnel
                                                   ^
                                    one outbound wss ("the trunk")
                                                   |
                             dcl-one-sdk start --tunnel wss://<tunnel-host>
                                                   |
                                       http://127.0.0.1:<port>  (local preview)
```

## Protocol (one trunk WebSocket)

Text frames are JSON control messages (discriminator `t`; unknown `t` ignored): `hello` / `welcome` / `open` / `open_ok` / `open_err` / `end` / `close` / `ping` / `pong`. Binary frames are channel data: `[u32 BE channel-id][u8 flags][payload]`, flags bit0 = payload was a binary ws message (preserves the text/binary distinction - load-bearing for the dual-frame live-reload channel). `kind:"ws"` channels: one data frame = one inner ws message; `kind:"http"` channels: data frames are body bytes terminated by `end`. Channel ids are allocated only by the service, monotonically per trunk. Exact grammar: `src/protocol.rs` (unit tests pin the wire strings) and the spec section 5.2.

- Subprotocol passthrough is mandatory: the comms upgrade negotiates `rfc5`/`rfc4`, the agent reports the locally selected one in `open_ok.subprotocol`, the public edge echoes it.
- Reconnect: agents present `resume {id, key}`; a disconnected id is held for `TUNNEL_GRACE_SECS` (120 s default), so the public URL survives blips. In-flight channels do not survive: pending HTTP answers 502, public websockets close 1012.

## Configuration (env, `Config::from_env`)

| var | default | meaning |
|---|---|---|
| `HTTP_SERVER_HOST` / `HTTP_SERVER_PORT` | `127.0.0.1` / `5167` | bind address |
| `PUBLIC_BASE_URL` | `http://<host>:<port>` | public origin used to mint `welcome.public_url` (`<base>/t/<id>`) |
| `TUNNEL_TOKENS` | empty = open | comma-separated bearer tokens; mismatch -> ws close 4401 |
| `TUNNEL_ALLOW_IDS` | empty = random ids | pinned id pool for stable URLs; exhausted -> close 4409 |
| `TUNNEL_GRACE_SECS` | 120 | disconnected-id retention |
| `TUNNEL_PING_SECS` | 20 | trunk ping interval (drop after 3 silent intervals) |
| `TUNNEL_OPEN_TIMEOUT_SECS` | 15 | `open` -> `open_ok` deadline (504 past it) |
| `TUNNEL_BODY_MAX_BYTES` | 67108864 | public request-body cap (413 past it); responses stream unbounded |

Deploying: one ws-capable nginx location on any https vhost is enough - everything, including the agent trunk at `/t/_connect`, lives under `/t/`. A ready vhost snippet + systemd unit template exist in the reference deployment's nginx conf.d / systemd config. Load-bearing directives: `proxy_http_version 1.1` + Upgrade/Connection passthrough, `proxy_read_timeout 1h`, `proxy_buffering off`, `client_max_body_size 0`.

Tests: `cargo test -p catalyrst-preview-tunnel` - protocol grammar + codec units, plus `tests/tunnel_flow.rs` integration (http multiplexing, ws subprotocol negotiation + text/binary preservation, 404/502/504 mapping, `open_err` -> 502, token 4401, resume-keeps-id, allow-ids pinning + 4409). Cross-crate end-to-end (real agent, real comms relay, real service): `dcl-one-sdk/tests/tunnel_e2e.rs`.
