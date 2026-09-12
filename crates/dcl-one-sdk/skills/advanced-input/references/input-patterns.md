# Advanced Input — Branch-Specific Patterns

Worked flows for the `advanced-input` skill. Basics (isTriggered/isPressed, InputModifier, PointerLock, PrimaryPointerInfo, the InputAction/event-type tables, troubleshooting) stay in SKILL.md.

## Per-Entity Input Command Cookbook (Tag-based)

Use the Tag component to mark all entities that share a same interaction, then iterate over them in a system. (Single-entity `getInputCommand` and `IA_ANY` basics: SKILL.md.)

```typescript
import { engine, inputSystem, InputAction, PointerEventType, Tags } from '@dcl/sdk/ecs'

function myInputSystem() {

  // fetch entities with a particular tag
  const taggedEntities = engine.getEntitiesByTag('myTag')
  
  // iterate over those entities
	for (const entity of taggedEntities) {
         // Check for click on a specific entity
        const clickData = inputSystem.getInputCommand(
          InputAction.IA_POINTER,
          PointerEventType.PET_DOWN,
          entity
        )

        if (clickData) {
          console.log('Entity clicked via system:', clickData.hit.entityId)
        }
    }

}

engine.addSystem(myInputSystem)
```

## Cutscene Pattern (freeze player during a cinematic)

```typescript
function startCutscene() {
  // Freeze player
  InputModifier.create(engine.PlayerEntity, {
    mode: InputModifier.Mode.Standard({ disableAll: true })
  })

  // ... play cinematic with VirtualCamera ...

  // After cutscene ends, restore movement
  // InputModifier.deleteFrom(engine.PlayerEntity)
}
```

## WASD Movement Pattern (drive a custom entity)

```typescript
import { engine, inputSystem, InputAction, PointerEventType, Transform } from '@dcl/sdk/ecs'
import { Vector3 } from '@dcl/sdk/math'

const MOVE_SPEED = 5

function customMovementSystem(dt: number) {
  const transform = Transform.getMutable(controllableEntity)
  let moveX = 0
  let moveZ = 0

  if (inputSystem.isPressed(InputAction.IA_FORWARD)) moveZ += 1
  if (inputSystem.isPressed(InputAction.IA_BACKWARD)) moveZ -= 1
  if (inputSystem.isPressed(InputAction.IA_LEFT)) moveX -= 1
  if (inputSystem.isPressed(InputAction.IA_RIGHT)) moveX += 1

  transform.position.x += moveX * MOVE_SPEED * dt
  transform.position.z += moveZ * MOVE_SPEED * dt
}

engine.addSystem(customMovementSystem)
```

WASD keys (`IA_FORWARD`, etc.) also control player movement — polling them reads the movement state but does not override it. To make WASD drive a custom entity instead of the avatar, freeze the avatar with `InputModifier`.

## Action Bar with Number Keys

```typescript
function actionBarSystem() {
  if (inputSystem.isTriggered(InputAction.IA_ACTION_3, PointerEventType.PET_DOWN)) {
    console.log('Slot 1 activated')
    useAbility(1)
  }
  if (inputSystem.isTriggered(InputAction.IA_ACTION_4, PointerEventType.PET_DOWN)) {
    console.log('Slot 2 activated')
    useAbility(2)
  }
  if (inputSystem.isTriggered(InputAction.IA_ACTION_5, PointerEventType.PET_DOWN)) {
    console.log('Slot 3 activated')
    useAbility(3)
  }
  if (inputSystem.isTriggered(InputAction.IA_ACTION_6, PointerEventType.PET_DOWN)) {
    console.log('Slot 4 activated')
    useAbility(4)
  }
}

engine.addSystem(actionBarSystem)
```

