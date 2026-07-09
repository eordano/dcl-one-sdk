import { engine, Transform } from '@dcl/sdk/ecs'
import { Vector3 } from '@dcl/sdk/math'
import { initAssetPacks } from '@dcl/asset-packs/dist/scene-entrypoint'

// {{TITLE}} — scaffolded by dcl-one-sdk init.
// initAssetPacks runs the no-code Actions/Triggers (smart items) authored in the
// editor; without it they decode but never execute. (The upstream toolchain only
// auto-injects it for the assets/scene/main.composite layout, and this project
// keeps the composite at the root.)
initAssetPacks(engine)

export function main() {
  const root = engine.addEntity()
  Transform.create(root, { position: Vector3.create(8, 0, 8) })
}
