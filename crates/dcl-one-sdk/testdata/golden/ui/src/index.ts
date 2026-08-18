import ReactEcs, { ReactEcsRenderer, UiEntity } from '@dcl/sdk/react-ecs'

// `.ts`, not `.tsx`: entrypoint::generate resolves exactly one user entry,
// src/index.ts, so a .tsx entry does not build. createElement is the same
// react-ecs surface JSX compiles down to.
const ui = () =>
  ReactEcs.createElement(
    UiEntity,
    {
      uiTransform: { width: 200, height: 40, margin: { top: 8, left: 8 } },
      uiText: { value: 'golden', fontSize: 16 },
    },
    null
  )

export function main() {
  ReactEcsRenderer.setUiRenderer(ui)
}
