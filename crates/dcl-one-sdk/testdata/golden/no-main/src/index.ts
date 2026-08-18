import { engine, MeshRenderer, Transform } from '@dcl/sdk/ecs'
import { Vector3 } from '@dcl/sdk/math'

// No exported main(): the generated entrypoint skips the startup system
// entirely, so this fixture pins what a scene that runs at module scope emits.
const sphere = engine.addEntity()
Transform.create(sphere, { position: Vector3.create(6, 1, 6) })
MeshRenderer.setSphere(sphere)
