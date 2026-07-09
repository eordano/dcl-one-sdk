# {{TITLE}}

{{DESCRIPTION}}

- **Parcels:** 0,0
- **Base:** 0,0

## Develop

```bash
npm install
npm run start
```

`npm run start` uses the npm toolchain (`@dcl/sdk-commands`); `dcl-one-sdk start`
runs the same preview from the Rust toolchain. Both work on this project.

## Publish

```bash
npm run deploy
```

Or `dcl-one-sdk deploy --target-content <content-server-url>`.
