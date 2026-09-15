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
