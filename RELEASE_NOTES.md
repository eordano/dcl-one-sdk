# dcl-one-sdk v0.26.1

The vendored SDK moves to the `auth-server` line, so `isServer`, `registerMessages`, `getRoom` and `@dcl/sdk/server` build and run out of the box, and the scaffold, blob and preview get leaner.

- Vendor `@dcl/sdk`, `@dcl/ecs` and `@dcl/js-runtime` 7.29.1-34986384248.commit-bb45080 (upstream's `auth-server` dist-tag): `isServer()`, `registerMessages`/`getRoom` rooms over the binary message bus, `@dcl/sdk/server` `Storage`/`EnvVar`, `CreatedBy` and authoritative component puts, and the network entity-delete framing fix now carried upstream.
- Run upstream's room protocol end to end in the preview: the host isolate answers `isServer`, stamps inbound frames with their verified sender and feeds the scene a live `RealmInfo`; the client loader presents the host as `authoritative-server`. The interim JSON room envelope is gone.
- Keep serverless-multiplayer scenes syncing: a scene without `authoritativeMultiplayer` trusts its peers as before; a flagged scene runs upstream's server-only trust untouched.
- Shrink the scaffold's `node_modules`: ship only the ECMAScript TypeScript libraries the scene tsconfig can reach (no DOM, no bundles) and `long`'s single build the runtime loads: 404 files and 9.87 MB unpacked (from 423 and 13.0 MB), 1.94 MB zipped (from 2.43 MB), which is also what every `init` extracts and every release binary embeds.
- Type-check scaffolded scenes with `skipLibCheck`, halving the check's time on the scaffold.
- Encode composites the way the vendored `@dcl/ecs` now does: optional fields set to `0`, `false` or `""` are present (upstream #1582; only a missing or null value is absent), and a one-of field with no case selected serializes as case 0 instead of failing the build (#1570).
- Gzip text-like preview responses over 1 KB, so the scene runtime chunk crosses the tunnel at about a third of its size (497 KB to 178 KB).

Includes the embedded LiveKit v1.13.7 and abgen servers. Builds use Rust 1.97.0 and the committed Cargo.lock.

# dcl-one-sdk v0.26.0

Server-side scenes get a storage service in the preview, matching the production `world-storage-service` route by route.

- Serve `Storage`, `Storage.player` and `EnvVar` for `authoritativeMultiplayer` scenes from `.dcl-one/storage.sqlite`, with production's routes, bodies, error messages, pagination, per-scope size limits and `/usage` endpoints.
- Add a Storage tab to the preview: scene, player and environment values with in-place edits, who wrote each value, recent activity, export and import.
- Add a "Use upstream storage" switch (and `storage target`) pointing the same routes at storage.decentraland.org, .zone or a custom service URL, forwarded with ADR-44 signed-fetch headers.
- Add `dcl-one-sdk storage scene|player|env get|set|delete|list|clear`, `target`, `export` and `import`, taking `sdk-commands storage`'s verbs and flags.
- Port the auth-server client into the host isolate: read cache with negative entries, coalesced reads, per-key write queues, cache seeding from listings, silent 404s.
- Say on `start` which storage the scene uses and where to switch it.

Includes the embedded LiveKit v1.13.7 and abgen servers. Builds use Rust 1.97.0 and the committed Cargo.lock.

# dcl-one-sdk v0.25.1

Publishing now makes the selected destination and existing World scenes easier to inspect and manage.

- Show parcel coordinates, scene footprints, publish outcomes and entrance controls on the target page.
- Remove individual World scenes through wallet signing, with a fresh deployment check before deletion.
- Separate files to upload from content already on the server, with file counts, sizes and progress bars.
- Improve target selection and LAND placement navigation.
- Share header, typography and theme styles across Preview, Scene, Target and Deploy.
- Update the Deploy header immediately when a wallet connects.
- Show media hostnames in a full-width row only when external media is enabled.
- Shorten the preview voice status to its comms endpoint and actual scene room IDs; simplify network address labels.

Includes the embedded LiveKit v1.13.7 and abgen servers. Builds use Rust 1.97.0 and the committed Cargo.lock.
