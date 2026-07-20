import { engine, Transform, MeshRenderer } from '@dcl/sdk/ecs'
import { Vector3 } from '@dcl/sdk/math'

// {{TITLE}} — scaffolded by dcl-one-sdk init.
export function main() {
  const cube = engine.addEntity()
  Transform.create(cube, { position: Vector3.create(8, 1, 8) })
  MeshRenderer.setBox(cube)
}
