import { engine, MeshRenderer, Transform } from '@dcl/sdk/ecs'
import { Vector3 } from '@dcl/sdk/math'

// Built with --custom-entry-point, so nothing generates a startup system and
// the SDK's own onStart/onUpdate come from this file's re-exports.
export * from '@dcl/sdk'

const cone = engine.addEntity()
Transform.create(cone, { position: Vector3.create(1, 1, 1) })
MeshRenderer.setCylinder(cone, 0.5, 0.5)
