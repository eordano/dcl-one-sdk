# Smart-Item Triggers → SDK7

All 19 `TriggerType` values and all 9 `TriggerConditionType` values from
`@dcl/asset-packs` (verified — `dist/trigger-enums.d.ts` in `@dcl/asset-packs` 2.17.2
declares exactly 19 and 9), with the SDK7 they compile down to. Verified against
`@dcl/sdk` 7.25.

A trigger in the palette is `event → conditions → actions`. In code it is a callback with
an early return. There is no registry, no event bus, no action queue: register the callback
where the entity is built.

The event mechanisms themselves (`pointerEventsSystem`, `TriggerArea`, `inputSystem`) are
owned by [[add-interactivity]] and [[advanced-input]] — this reference says which one each
palette trigger maps to and what the palette layered on top. For the action half see
`{baseDir}/references/actions.md`; for the method and the composition patterns see
`{baseDir}/SKILL.md`.

| Trigger | Mechanism |
| --- | --- |
| [ON_CLICK](#on_click) | `pointerEventsSystem.onPointerDown` |
| [ON_INPUT_ACTION](#on_input_action) | same, non-pointer button |
| [ON_GLOBAL_CLICK / PRIMARY / SECONDARY](#on_global_click-on_global_primary-on_global_secondary) | `inputSystem.isTriggered` in a system |
| [ON_CLICK_IMAGE](#on_click_image) | react-ecs `onMouseDown` |
| [ON_PLAYER_ENTERS_AREA / LEAVES_AREA](#on_player_enters_area-on_player_leaves_area) | `TriggerArea` + `triggerAreaEventsSystem` |
| [ON_TICK](#on_tick) | `engine.addSystem` |
| [ON_DELAY](#on_delay) | `timers.setTimeout` |
| [ON_LOOP](#on_loop) | `timers.setInterval` |
| [ON_TWEEN_END](#on_tween_end) | `tweenSystem.tweenCompleted` polled in a system |
| [ON_SPAWN](#on_spawn) | the factory function body |
| [ON_CLONE](#on_clone) | the clone function body |
| [ON_PLAYER_SPAWN](#on_player_spawn) | `Transform` on `engine.PlayerEntity`, or `onEnterScene` |
| [ON_STATE_CHANGE](#on_state_change) | your `setState` |
| [ON_COUNTER_CHANGE](#on_counter_change) | your counter mutator |
| [ON_DAMAGE](#on_damage) | your damage function |
| [ON_HEAL_PLAYER](#on_heal_player) | your heal function |

---

## ON_CLICK

Left click / primary pointer on the entity. Needs a pointer collider
(`MeshCollider.setBox(entity)` or a `GltfContainer` with `visibleMeshesCollisionMask`
including `CL_POINTER`), otherwise the ray passes through and nothing fires.

```ts
pointerEventsSystem.onPointerDown(
  { entity, opts: { button: InputAction.IA_POINTER, hoverText: 'Open', maxDistance: 10 } },
  () => open(entity)
)
```

Full API and pitfalls (never re-register a handler from inside its own callback; skinned
GLBs need an explicit child collider): [[add-interactivity]].

`opts`: `hoverText` is the crosshair tooltip (default `'Interact'`), `maxDistance` is the
camera-to-entity range (default 10), `maxPlayerDistance` the avatar-to-entity range,
`showFeedback: false` hides tooltip and highlight, `showHighlight: false` hides only the
highlight, `priority` breaks ties when events overlap. The `button` default is `IA_ANY`;
name it explicitly.

Removing it — this is also how you make a one-shot trigger:

```ts
pointerEventsSystem.removeOnPointerDown(entity)
```

`onPointerUp`, `onPointerHoverEnter`, `onPointerHoverLeave` exist with the same shape and
have no palette equivalent.

## ON_INPUT_ACTION

The same mechanism bound to a key rather than the pointer. The palette exposes this as a
separate trigger only because its default is `IA_PRIMARY`.

```ts
pointerEventsSystem.onPointerDown(
  { entity, opts: { button: InputAction.IA_PRIMARY, hoverText: 'Press E' } },
  () => activate(entity)
)
```

The full `InputAction` enum: `IA_POINTER` (left click), `IA_PRIMARY` (E), `IA_SECONDARY`
(F), `IA_ANY`, `IA_FORWARD`, `IA_BACKWARD`, `IA_RIGHT`, `IA_LEFT`, `IA_JUMP`, `IA_WALK`,
`IA_ACTION_3`..`IA_ACTION_6` (number keys 1-4), `IA_MODIFIER` (shift). There is no
`IA_ACTION_1`/`IA_ACTION_2`.

## ON_GLOBAL_CLICK, ON_GLOBAL_PRIMARY, ON_GLOBAL_SECONDARY

Fire anywhere, not on an entity. Poll the input system once per frame.

```ts
function globalInputSystem() {
  if (inputSystem.isTriggered(InputAction.IA_POINTER, PointerEventType.PET_DOWN)) {
    onGlobalClick()
  }
  if (inputSystem.isTriggered(InputAction.IA_PRIMARY, PointerEventType.PET_DOWN)) {
    onGlobalPrimary()
  }
  if (inputSystem.isTriggered(InputAction.IA_SECONDARY, PointerEventType.PET_DOWN)) {
    onGlobalSecondary()
  }
}
engine.addSystem(globalInputSystem)
```

Polling patterns in full (held keys, combos, cursor lock): [[advanced-input]].

`isTriggered(button, type)` is true only on the frame the event arrived.
`isPressed(button)` stays true while the button is held.
`getInputCommand(button, type, entity?)` returns the `PBPointerEventsResult` payload
(including `hit`, with the world position and normal of the ray hit) or null, optionally
scoped to one entity.

A global click also fires when the player clicks an entity, so guard for it if you have
both.

## ON_CLICK_IMAGE

Fires when the player clicks the image drawn by the SHOW_IMAGE action. In react-ecs it is
the element's own handler.

```tsx
<UiEntity
  uiTransform={{ width: 512, height: 320, pointerFilter: 'block' }}
  uiBackground={{ textureMode: 'stretch', texture: { src } }}
  onMouseDown={() => onImageClick()}
/>
```

Full React-ECS API: [[build-ui]]. `pointerFilter: 'block'` is required on the clickable
element, and the containing
full-screen layer should be `pointerFilter: 'none'` so it does not eat world clicks.
`Button` from `@dcl/sdk/react-ecs` wraps the same thing with a styled default.

## ON_PLAYER_ENTERS_AREA, ON_PLAYER_LEAVES_AREA

`TriggerArea` shapes an intangible volume from the entity's Transform (position, rotation,
scale). The events arrive through `triggerAreaEventsSystem`. Full component reference,
including the `result.trigger?.entity` vs `result.triggeredEntity` gotcha:
[[add-interactivity]].

```ts
Transform.create(area, { position: Vector3.create(8, 1, 8), scale: Vector3.create(4, 3, 4) })
TriggerArea.setBox(area, ColliderLayer.CL_MAIN_PLAYER)

triggerAreaEventsSystem.onTriggerEnter(area, () => open(door))
triggerAreaEventsSystem.onTriggerExit(area, () => close(door))
```

Notes:

- Do better than the smart item here. Asset-packs calls `TriggerArea.setBox(entity)` with
  the default `CL_PLAYER` layer, which fires for *every* avatar, and then discards the
  non-local ones in JS. `CL_MAIN_PLAYER` (verified — `@dcl/ecs` `mesh_collider.gen.d.ts`:
  `CL_MAIN_PLAYER = 8`, "layer corresponding to the local (main) player avatar") is the
  local player only, is short-circuited by the physics engine, and needs no filtering.
- If you do want all avatars, keep `CL_PLAYER` and compare in the handler:
  `if (result.trigger?.entity !== engine.PlayerEntity) return`.
- `TriggerArea.setSphere(area, ColliderLayer.CL_MAIN_PLAYER)` for a sphere; the engine
  prefers spheres, they are a distance check. `Transform.scale` sizes either shape.
- Any layer works as the mask (`CL_CUSTOM1` etc.) for non-avatar triggers — a ball entering
  a goal.
- `onTriggerStay` fires every frame while inside; no palette equivalent, easy to misuse.
  Remove handlers with `removeOnTriggerEnter` / `removeOnTriggerStay` / `removeOnTriggerExit`.
- To see the volume while developing, add `MeshRenderer.setBox(area)` — the default mesh
  matches the trigger dimensions exactly.

For "near an entity" rather than "inside a box", `pointerEventsSystem.onProximityEnter` /
`onProximityLeave` is the simpler tool.

## ON_TICK

Every frame.

```ts
function tickSystem(dt: number) {
  // dt is seconds since the last frame
}
engine.addSystem(tickSystem)
engine.removeSystem(tickSystem)
```

Systems can be prioritised: `engine.addSystem(fn, 10, 'name')`, higher runs first. See
[[scene-runtime]] for system execution order.

Keep per-frame work small. Most of what the palette used ON_TICK for (polling a distance,
polling a state) is better as a trigger area or a callback.

## ON_DELAY

Fires when a START_DELAY elapses. In code the callback body *is* the trigger.

```ts
const id = timers.setTimeout(() => onDelayElapsed(), 3000)
timers.clearTimeout(id)     // the palette's STOP_DELAY
```

## ON_LOOP

Fires on each iteration of a START_LOOP.

```ts
const id = timers.setInterval(() => onEachIteration(), 5000)
timers.clearInterval(id)
```

## ON_TWEEN_END

There is no callback. `tweenSystem.tweenCompleted(entity)` returns true on the single frame
the tween finishes; poll it in a system. Tween API in full: [[animations-tweens]].

```ts
const listeners = new Map<Entity, () => void>()

function tweenEndSystem() {
  for (const [entity, cb] of listeners) {
    if (Tween.has(entity) && tweenSystem.tweenCompleted(entity)) cb()
  }
}
engine.addSystem(tweenEndSystem)
```

Or drive a state machine directly off it, which is usually what the palette was doing:

```ts
function doorSystem() {
  for (const [door, state] of engine.getEntitiesWith(DoorState)) {
    if (state.value !== 'opening' && state.value !== 'closing') continue
    if (!Tween.has(door) || !tweenSystem.tweenCompleted(door)) continue
    DoorState.getMutable(door).value = state.value === 'opening' ? 'open' : 'closed'
  }
}
```

Caveat: a looping `TweenSequence` never "completes" in the sense the palette means. Chain
your own follow-up tween instead of waiting for an end event.

## ON_SPAWN

Fires once when the item is created — which, in code, is the factory function body. There
is nothing to register.

```ts
function createBarrel(position: Vector3): Entity {
  const entity = engine.addEntity()
  Transform.create(entity, { position })
  GltfContainer.create(entity, { src: 'models/barrel.glb' })
  // everything ON_SPAWN would have run:
  onClick(entity, 'Break', () => breakBarrel(entity))
  return entity
}
```

For entities placed in the Creator Hub editor rather than created in code, the equivalent
moment is inside `main()` — `getEntityOrNullByName` / `getEntitiesByTag` are the composite
lookups documented in [[composites]] and [[add-interactivity]]:

```ts
export function main() {
  const barrel = engine.getEntityOrNullByName('barrel1')
  if (barrel) onClick(barrel, 'Break', () => breakBarrel(barrel))
}
```

## ON_CLONE

Fires on the copy produced by CLONE_ENTITY. Same idea: it is the tail of your clone
function.

```ts
function cloneBarrel(source: Entity): Entity {
  const clone = cloneEntity(source)
  onClick(clone, 'Break', () => breakBarrel(clone))   // rebind per-entity behaviour
  return clone
}
```

This is the one place the generic clone genuinely loses information: pointer handlers and
timers are closures over the original entity, not components, so they are not copied. You
must rebind them. A factory function avoids the problem entirely.

## ON_PLAYER_SPAWN

For the local player, the moment its `Transform` becomes readable.

```ts
let seen = false
function playerSpawnSystem() {
  if (seen) return
  const transform = Transform.getOrNull(engine.PlayerEntity)
  if (!transform) return
  seen = true
  onPlayerSpawn(transform.position)
  engine.removeSystem(playerSpawnSystem)
}
engine.addSystem(playerSpawnSystem)
```

For other players entering or leaving the scene:

```ts
import { onEnterScene, onLeaveScene } from '@dcl/sdk/players'

onEnterScene((player) => console.log('enter', player.userId, player.name))
onLeaveScene((userId) => console.log('leave', userId))
```

`getPlayer()` returns the local player's data (`userId`, `name`, `isGuest`, `avatar`,
`wearables`, `emotes`, `position`, `entity`) or null. Full player API: [[player-avatar]].

## ON_STATE_CHANGE

Fires when SET_STATE changes the value. Route every write through one function and call the
listeners there — see `{baseDir}/references/actions.md` → SET_STATE.

```ts
type StateListener = (next: string, prev: string) => void
const stateListeners = new Map<Entity, StateListener[]>()

function onStateChange(entity: Entity, cb: StateListener) {
  const list = stateListeners.get(entity) ?? []
  list.push(cb)
  stateListeners.set(entity, list)
}
```

For a single-consumer case, skip the listener list and call the follow-up directly from
`setState`. The list only earns its keep when several unrelated things watch one entity.

## ON_COUNTER_CHANGE

Same shape, fired from SET_COUNTER / INCREMENT_COUNTER / DECREASE_COUNTER.

```ts
function setCounter(entity: Entity, value: number) {
  Counter.getMutable(entity).value = value
  for (const cb of counterListeners.get(entity) ?? []) cb(value)
}
```

The common use — "when the counter reaches N, do X" — is a threshold check inside the
mutator, which is also the palette's WHEN_COUNTER_EQUALS condition:

```ts
function addCounter(entity: Entity, amount = 1) {
  const next = Counter.get(entity).value + amount
  setCounter(entity, next)
  if (next >= 3) unlock(entity)
}
```

## ON_DAMAGE

No SDK event. Fired by your own DAMAGE implementation — see
`{baseDir}/references/actions.md` → Damage and health.

```ts
function damageInRadius(origin: Entity, radius: number, hits = 1) {
  // ...
  for (const cb of damageListeners.get(target) ?? []) cb(hits)
}
```

The palette fires ON_DAMAGE once per hit, so a `hits: 3` damage action fires the trigger
three times. Reproduce that only if you actually want it.

## ON_HEAL_PLAYER

Same, from your HEAL_PLAYER implementation. There is no avatar health in Decentraland;
this only ever meant "a number the scene owns went up".

---

## Trigger conditions

The palette lets a trigger carry a list of conditions combined with AND or OR. In code
these are guard clauses at the top of the handler. `AND` is sequential early returns or
`&&`; `OR` is `||`.

| Condition | Code |
| --- | --- |
| `WHEN_STATE_IS` | `State.getOrNull(e)?.current === 'open'` |
| `WHEN_STATE_IS_NOT` | `State.getOrNull(e)?.current !== 'open'` |
| `WHEN_PREVIOUS_STATE_IS` | `State.getOrNull(e)?.previous === 'closed'` |
| `WHEN_PREVIOUS_STATE_IS_NOT` | `State.getOrNull(e)?.previous !== 'closed'` |
| `WHEN_COUNTER_EQUALS` | `Counter.get(e).value === 3` |
| `WHEN_COUNTER_IS_GREATER_THAN` | `Counter.get(e).value > 3` |
| `WHEN_COUNTER_IS_LESS_THAN` | `Counter.get(e).value < 3` |
| `WHEN_DISTANCE_TO_PLAYER_LESS_THAN` | `distanceToPlayer(e) < 5` |
| `WHEN_DISTANCE_TO_PLAYER_GREATER_THAN` | `distanceToPlayer(e) > 5` |

```ts
function distanceToPlayer(entity: Entity): number {
  const player = Transform.getOrNull(engine.PlayerEntity)
  if (!player) return Infinity
  return Vector3.distance(worldPosition(entity), player.position)
}

onClick(chest, 'Open', () => {
  if (State.getOrNull(chest)?.current !== 'locked') return
  if (Counter.get(chest).value < 3) return
  if (distanceToPlayer(chest) > 5) return
  open(chest)
})
```

Note that the conditions may be evaluated against a *different* entity than the one the
trigger is on — in the GUI you pick which item's counter to read. In code that is just
naming the other entity.

Two conditions that only exist in the palette because it had no expression language, and
which you should not bother reproducing structurally: an OR of two counter comparisons is
`a > 3 || a < 1`, and a distance check is better replaced by a `TriggerArea` when it is
being polled every frame.
