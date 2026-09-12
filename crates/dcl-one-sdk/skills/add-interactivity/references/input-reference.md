# Input System Reference

## All Input Actions

| Action            | Key Binding       | Constant                   |
| ----------------- | ----------------- | -------------------------- |
| Left mouse button | Mouse click / tap | `InputAction.IA_POINTER`   |
| Primary action    | E key             | `InputAction.IA_PRIMARY`   |
| Secondary action  | F key             | `InputAction.IA_SECONDARY` |
| Action 3          | 1 key             | `InputAction.IA_ACTION_3`  |
| Action 4          | 2 key             | `InputAction.IA_ACTION_4`  |
| Action 5          | 3 key             | `InputAction.IA_ACTION_5`  |
| Action 6          | 4 key             | `InputAction.IA_ACTION_6`  |
| Jump              | Space key         | `InputAction.IA_JUMP`      |
| Forward           | W key             | `InputAction.IA_FORWARD`   |
| Backward          | S key             | `InputAction.IA_BACKWARD`  |
| Left              | A key             | `InputAction.IA_LEFT`      |
| Right             | D key             | `InputAction.IA_RIGHT`     |
| Walk              | Control key       | `InputAction.IA_WALK`      |
| Run               | Shift key         | `InputAction.IA_MODIFIER`  |
| Any (wildcard)    | Any of the above  | `InputAction.IA_ANY`       |

**Notes:**

- Mouse wheel is **not available** as an input
- Always design for both desktop and mobile — mobile has no keyboard, rely on pointer and on-screen buttons
- Set `maxDistance` on pointer events (8-10 meters typical) to prevent interactions from across the scene
- Use `hoverText` to communicate what an interaction does before the player commits

## Declarative Pointer Events Component

Instead of the callback system, you can use the `PointerEvents` component directly:

```typescript
import { PointerEvents, PointerEventType, InputAction } from "@dcl/sdk/ecs";

PointerEvents.create(entity, {
  pointerEvents: [
    {
      eventType: PointerEventType.PET_DOWN,
      eventInfo: {
        button: InputAction.IA_POINTER,
        hoverText: "Click me",
        showFeedback: true,
        maxDistance: 10,
      },
    },
  ],
});
```

Then read results in a system using `inputSystem.getInputCommand()`.

## Proximity Interactions

Detect button events when the player is near and roughly facing an entity, without requiring them to aim the cursor at it. Unlike pointer events (which raycast), proximity events check a wide triangular slice of a sphere projecting forward from the avatar — avatar facing matters, independently of camera direction. Code examples: `{baseDir}/references/interactivity-patterns.md`.

### Options

| Option              | Description                                                                                                              |
| ------------------- | ------------------------------------------------------------------------------------------------------------------------ |
| `button`            | Which button to listen for (`InputAction.IA_PRIMARY`, `IA_SECONDARY`, `IA_POINTER`, etc.)                                |
| `maxPlayerDistance` | Max distance from the player's **avatar** to the entity (meters). This is the most relevant option for proximity events. |
| `maxDistance`       | Max distance from the player's **camera** to the entity (meters).                                                        |
| `hoverText`         | Text shown in the UI when the player is in range.                                                                        |
| `showHighlight`     | Show an edge highlight on the entity when player is in range. Default: `true`.                                           |
| `showFeedback`      | Show hover feedback around the center of the entity. Default: `true`.                                                    |
| `priority`          | Conflict resolution when multiple entities are in range. Higher values respond first.                                    |

## Raycast Direction Types

```typescript
// 1. Local direction — relative to entity rotation
{ $case: 'localDirection', localDirection: Vector3.Forward() }

// 2. Global direction — world-space direction, ignores entity rotation
{ $case: 'globalDirection', globalDirection: Vector3.Down() }

// 3. Global target — aim at a specific world position
{ $case: 'globalTarget', globalTarget: Vector3.create(10, 0, 10) }

// 4. Target entity — aim at another entity dynamically
{ $case: 'targetEntity', targetEntity: entityId }
```

### Raycast Options

```typescript
{
  direction: Vector3.Forward(),
  maxDistance: 16,
  queryType: RaycastQueryType.RQT_HIT_FIRST,  // first hit (NOT necessarily closest); RQT_QUERY_ALL = all; RQT_NONE = skip
  originOffset: Vector3.create(0, 0.5, 0),     // offset from entity world origin (parent chain applied)
  collisionMask: ColliderLayer.CL_PHYSICS | ColliderLayer.CL_CUSTOM1,
  continuous: false  // true = every frame, false = one-shot
}
```

## Avatar Modifier Areas

Modify how avatars appear or behave in a region:

```typescript
import { AvatarModifierArea, AvatarModifierType } from "@dcl/sdk/ecs";

AvatarModifierArea.create(entity, {
  area: Vector3.create(4, 3, 4),
  modifiers: [AvatarModifierType.AMT_HIDE_AVATARS],
  excludeIds: ["0x123...abc"], // Optional
});

// Available modifiers:
// AMT_HIDE_AVATARS      — Hide all avatars in the area
// AMT_DISABLE_PASSPORTS — Disable clicking on avatars to see profiles
// To disable jumping in an area, use InputModifier's `disableJump` flag
// (covered in the advanced-input skill), not an AvatarModifierType.
```

## Trigger Area Callback Fields

The trigger area event callback receives a `DeepReadonlyObject<PBTriggerAreaResult>`.

**Top-level — the trigger area itself (the entity whose volume was activated):**
- `triggeredEntity` — The trigger area's own entity. Comparing this to `engine.PlayerEntity` is always true and the guard never fires — do NOT use this for the local-player check.
- `triggeredEntityPosition` — World position of the trigger area entity
- `triggeredEntityRotation` — World rotation of the trigger area entity
- `eventType` — `TAET_ENTER` (0), `TAET_STAY` (1), or `TAET_EXIT` (2)
- `timestamp` — Tick timestamp

**Nested `trigger: { ... }` — the entity that entered/exited the volume:**
- `trigger.entity` — The entity that entered the area (compare against `engine.PlayerEntity` to filter to local player)
- `trigger.layers` — The collider layers the area listens for
- `trigger.position` — World position of the entity that entered
- `trigger.rotation` — World rotation of the entity that entered
- `trigger.scale` — World scale of the entity that entered
