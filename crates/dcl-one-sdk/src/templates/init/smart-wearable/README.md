# {{TITLE}}

{{DESCRIPTION}}

## Before you publish

This scaffold ships no binary assets - add them at the project root:

- `model.glb` - the wearable model `wearable.json` references
  (`data.representations[0].mainFile` / `contents`); update both fields if your
  file has a different name.
- `thumbnail.png` - 256x256 PNG with a transparent background, shown in the
  marketplace and the backpack.

`wearable.json` fields to review: `name`, `description`, `category` (eyewear,
hat, upper_body, ...), `rarity` (unique, mythic, legendary, epic, rare,
uncommon, common), `data.tags`, and `data.hides` / `data.replaces`.

## Develop

```bash
npm install
npm run start
```

(or `dcl-one-sdk start` with the Rust toolchain)

## Pack

```bash
npm run pack
```

Produces `smart-wearable.zip` for the builder upload - keep it under 2 MiB.
