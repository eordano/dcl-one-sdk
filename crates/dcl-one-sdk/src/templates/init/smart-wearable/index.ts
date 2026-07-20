import { engine, Transform } from '@dcl/sdk/ecs'
import { Vector3 } from '@dcl/sdk/math'

// {{TITLE}} — scaffolded by dcl-one-sdk init --project smart-wearable.
// This code runs as a portable experience whenever the wearable is equipped.
export function main() {
  const root = engine.addEntity()
  Transform.create(root, { position: Vector3.create(8, 0, 8) })
}
