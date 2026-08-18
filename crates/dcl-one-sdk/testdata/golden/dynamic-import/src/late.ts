import { Entity, TransformComponentExtended } from '@dcl/sdk/ecs'
import { Vector3 } from '@dcl/sdk/math'

export function place(entity: Entity, Transform: TransformComponentExtended): void {
  Transform.create(entity, { position: Vector3.create(2, 3, 4) })
}
