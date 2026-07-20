# {{TITLE}}

{{DESCRIPTION}}

- **Parcels:** 0,0
- **Base:** 0,0

## Develop

```bash
dcl-one-sdk start
```

node_modules ships with `dcl-one-sdk init` — no npm install needed. The npm
scripts (`npm start`, `npm run build`, `npm run deploy`) call `dcl-one-sdk`
from your PATH.

## Publish

```bash
npm run deploy
```

Or `dcl-one-sdk deploy --target-content <content-server-url>`.
