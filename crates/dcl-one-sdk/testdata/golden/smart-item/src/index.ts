import { EntityNames } from '../assets/scene/entity-names'
import { engine, Transform } from '@dcl/sdk/ecs'
import { Vector3 } from '@dcl/sdk/math'

// Importing the generated enum is the point: it makes the build's
// entity-names.ts step load-bearing, so a regression there fails the type
// check instead of leaving a stale file nobody reads.
export function main() {
  const box = engine.getEntityOrNullByName(EntityNames.Counter_Box)
  if (box) Transform.getMutable(box).position = Vector3.create(9, 1, 9)
}
