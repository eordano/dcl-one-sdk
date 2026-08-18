import { engine, Transform } from '@dcl/sdk/ecs'
import { Vector3 } from '@dcl/sdk/math'

// The composite places the geometry; scene code only moves what it placed, so
// the golden shows composite state and scene traffic side by side.
export function main() {
  const placed = engine.getEntityOrNullByName('nothing') ?? engine.addEntity()
  Transform.createOrReplace(placed, { position: Vector3.create(4, 2, 4) })
}
