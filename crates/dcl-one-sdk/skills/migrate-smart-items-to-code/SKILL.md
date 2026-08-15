---
name: migrate-smart-items-to-code
description: Port Creator Hub smart items — the no-code Actions/Triggers palette from `@dcl/asset-packs` — to plain SDK7 TypeScript. Covers all 63 ActionTypes, 19 TriggerTypes, and 9 TriggerConditionTypes with a verified SDK7 equivalent for each, plus the wiring the editor used to supply (delays, loops, counters, states, sequences). Use when the user wants a smart item's behavior in code, is extending a Creator-Hub-authored scene beyond what the palette allows, asks "how do I do X when Y" and X or Y is a palette entry, or names smart items, the Actions panel, no-code triggers, or `@dcl/asset-packs`. Do NOT use to author smart-item data in a composite (see composites), to write a Creator Hub Script component class (see script-components), or as the general reference for the underlying components — each section points at the topic skill that owns it.
---

# Migrate Creator Hub Smart Items to SDK7 Code

> **This is a porting skill.** It assumes a palette behavior that exists — in a Creator Hub
> scene, in a tutorial, or in the user's head — and turns it into TypeScript. For clickable
> objects and trigger zones from scratch, [[add-interactivity]] is the shorter path. For
> keeping the no-code data and editing it as composite JSON, see [[composites]].

Decentraland's smart items are a fixed no-code palette: 63 action types, 19 trigger types,
9 trigger conditions, wired in the Creator Hub GUI and interpreted at runtime by
`@dcl/asset-packs`. Every one of them is a thin wrapper over an SDK7 component or a
`~system/*` call. Writing the SDK7 directly is smaller, readable, debuggable, and not
capped by the palette.

This skill is the translation table plus the patterns the GUI's wiring used to supply.

- `{baseDir}/references/actions.md` — all 63 actions, minimal correct SDK7 for each.
- `{baseDir}/references/triggers.md` — all 19 triggers and 9 conditions.

## RULE: Read the reference entry before writing the code

Look the palette entry up in the reference and paste from it. Do NOT write DCL component
code from memory — component field names and helper signatures change between SDK minors,
and the smart item's parameter names are frequently *not* the SDK's field names (the
palette's durations are seconds; `Tween` and `timers` take milliseconds).

## RULE: Fetch composite entities — never re-create them

A smart item lives on an entity the user placed in the Creator Hub, so it is already in
`assets/scene/main.composite`. Look it up inside `main()`; do NOT `engine.addEntity()` a
second copy.

```typescript
import { engine, pointerEventsSystem, InputAction } from '@dcl/sdk/ecs'
import { EntityNames } from '../assets/scene/entity-names'

export function main() {
  const door = engine.getEntityOrNullByName(EntityNames.Door_1)
  if (!door) return
  pointerEventsSystem.onPointerDown(
    { entity: door, opts: { button: InputAction.IA_PRIMARY, hoverText: 'Open' } },
    () => openDoor(door)
  )
}
```

`engine.getEntitiesByTag('Crystal')` does the same for a group. Both must run inside
`main()` or later — composite entities do not exist before that. See [[add-interactivity]]
and [[composites]].

## RULE: Do not strip `@dcl/asset-packs` until the composite is clean

Removing `initAssetPacks` from `src/index.ts` is only half the job. If the composite still
carries `asset-packs::Actions`, `asset-packs::Triggers`, `asset-packs::States` or
`asset-packs::Counter`, the no-code behavior is still declared in the scene and (on older
SDKs) the package is still a build dependency. Remove the ported entities' asset-packs
components and their `Counter` bookkeeping together with the import, then build. **Leave
`asset-packs::ActionTypes` and every `inspector::*` component alone** — those are Creator
Hub-managed and deleting them breaks the editor view of the scene. The exact edit-mode
rules are in [[composites]].

If the user will keep opening the scene in the Creator Hub, the safe end state is: entities
and their placement stay in the composite, behavior moves to TypeScript, asset-packs data
is removed only for the entities you actually ported.

## Method

1. Name the trigger and the action(s). A smart item is `trigger → [actions]`.
2. Look each one up in the references. Every entry is one function you can paste.
3. Write a plain TypeScript function per action, and register the trigger in `main()`.
4. Add `requiredPermissions` to `scene.json` if any action needs one (table below).
5. Type-check with `npm run build`, then run `npm start` and exercise the behavior.

**Done when:** every trigger and action from the original item has a counterpart in code,
`npm run build` exits 0, and the ported entities no longer carry `asset-packs::Actions` /
`asset-packs::Triggers` / `asset-packs::States` in the composite. If the scene still calls
`initAssetPacks`, that is only correct while some *other* entity still uses the palette.

Do not reproduce asset-packs' architecture. It carries a generic action-dispatch engine
(`Actions`/`Triggers` components, string action names, an event bus, a queue) because it
interprets editor data at runtime. Code that is written, not interpreted, calls the
function directly.

## Decision tree

| Palette concept | SDK7 | Owned by |
| --- | --- | --- |
| ON_CLICK, ON_INPUT_ACTION | `pointerEventsSystem.onPointerDown` | [[add-interactivity]] |
| ON_PLAYER_ENTERS_AREA / LEAVES_AREA | `TriggerArea` + `triggerAreaEventsSystem` | [[add-interactivity]] |
| ON_GLOBAL_CLICK / PRIMARY / SECONDARY | `inputSystem.isTriggered` in a system | [[advanced-input]] |
| ON_TICK | `engine.addSystem` | [[scene-runtime]] |
| ON_DELAY / ON_LOOP, START_DELAY / START_LOOP | `timers.setTimeout` / `setInterval` | [[scene-runtime]] |
| PLAY_ANIMATION, START_TWEEN, ON_TWEEN_END | `Animator`, `Tween`, `tweenSystem` | [[animations-tweens]] |
| PLAY_SOUND, PLAY_VIDEO_STREAM | `AudioSource`, `AudioStream`, `VideoPlayer` | [[audio-video]] |
| CHANGE_CAMERA | `VirtualCamera` + `MainCamera` | [[camera-control]] |
| LIGHTS_ON/OFF/MODIFY, CHANGE_SKYBOX | `LightSource`, `SkyboxTime` | [[lighting-environment]] |
| MOVE_PLAYER, PLAY_*_EMOTE, ATTACH_TO_PLAYER | `movePlayerTo`, `triggerEmote`, `AvatarAttach` | [[player-avatar]] |
| FREEZE_PLAYER / UNFREEZE_PLAYER | `InputModifier` | [[advanced-input]] |
| SHOW_TEXT / SHOW_IMAGE, ON_CLICK_IMAGE | React-ECS renderer + module state | [[build-ui]] |
| SET_STATE, SET_COUNTER, ON_STATE_CHANGE, conditions | your own component + one mutator function | this skill |
| BATCH, RANDOM, sequences | plain function calls | this skill |
| SPAWN_ENTITY, CLONE_ENTITY | a factory function | this skill |
| CALL_SCRIPT_METHOD | an import and a call | [[script-components]] |

The rows marked *this skill* are the ones with no component behind them — they are the
parts people rebuild badly from scratch, and they are the reason this is one skill rather
than a note in each of the others.

## Scene skeleton

`scene.json` points `main` at the build output; `src/index.ts` exports `main()`, which the
runtime calls once after the scene loads.

```ts
// src/index.ts
import { engine, Transform, MeshRenderer, MeshCollider } from '@dcl/sdk/ecs'
import { Vector3 } from '@dcl/sdk/math'

export function main() {
  const cube = engine.addEntity()
  Transform.create(cube, { position: Vector3.create(8, 1, 8) })
  MeshRenderer.setBox(cube)
  MeshCollider.setBox(cube)
}
```

(Entities created in code like this are for runtime-spawned objects. Static scenery belongs
in `assets/scene/main.composite` — see the composite-first rule in [[sdk-scenes]].)

Rules that bite:

- Put initial scene code in `main()`. Module top level runs before the scene context is
  loaded, so the player entity and anything placed via the Creator Hub editor are not
  there yet. Code outside `main()` is fine when it is called by `main()`, when it is a
  system, or when it is inside an async function.
- `engine.defineComponent(...)` is the exception: custom component definitions must be at
  module level (conventionally their own file), evaluated before `main()` runs.
- `Component.get()` returns a frozen readonly value. Mutate through
  `Component.getMutable(entity)`; use `getMutableOrNull` when the component may be absent.
- `Component.create` throws if the component already exists. `createOrReplace` does not.
- A "system" is any `(dt: number) => void` registered with `engine.addSystem(fn)`. `dt` is
  in **seconds**. Tween and timer durations are in **milliseconds**.

## Worked example

"Make this cube play a sound when clicked, then teleport the player after 3 seconds."

```ts
import {
  engine, Transform, MeshRenderer, MeshCollider, AudioSource,
  InputAction, pointerEventsSystem, timers
} from '@dcl/sdk/ecs'
import { Vector3 } from '@dcl/sdk/math'
import { teleportTo } from '~system/RestrictedActions'

export function main() {
  const cube = engine.addEntity()
  Transform.create(cube, { position: Vector3.create(8, 1, 8) })
  MeshRenderer.setBox(cube)
  MeshCollider.setBox(cube)
  AudioSource.create(cube, { audioClipUrl: 'sounds/chime.mp3', playing: false })

  pointerEventsSystem.onPointerDown(
    { entity: cube, opts: { button: InputAction.IA_POINTER, hoverText: 'Activate' } },
    () => {
      AudioSource.playSound(cube, 'sounds/chime.mp3', true)
      timers.setTimeout(() => {
        void teleportTo({ worldCoordinates: { x: 72, y: -10 } })
      }, 3000)
    }
  )
}
```

That is ON_CLICK → PLAY_SOUND → START_DELAY → TELEPORT_PLAYER, four palette entries, in
one handler.

## Composition patterns

These are what the GUI's wiring actually bought people. They are the parts most often
written badly from scratch.

### Chain (BATCH): do A then B

Call them. There is no ordering primitive to reach for.

```ts
function onActivate() {
  openDoor()
  playChime()
  incrementScore()
}
```

### Delay (START_DELAY / STOP_DELAY)

`timers` is engine-bound and exported from `@dcl/sdk/ecs`. It ticks with the scene, so it
pauses when the scene pauses; bare global `setTimeout` is the same implementation
polyfilled onto `globalThis`.

```ts
import { timers } from '@dcl/sdk/ecs'

let pending: number | undefined
pending = timers.setTimeout(() => closeDoor(), 3000)
if (pending !== undefined) timers.clearTimeout(pending) // STOP_DELAY
```

Do not build a per-frame countdown system for this; that is what asset-packs does
internally only because it must key timers by entity and action name.

### Loop (START_LOOP / STOP_LOOP)

```ts
const loop = timers.setInterval(() => spawnWave(), 5000)
timers.clearInterval(loop)
```

`setInterval` fires first after one interval. The smart item fires immediately and then
every interval; if you need that, call the body once before arming the interval.

### Sequenced steps

```ts
function sequence(steps: Array<{ afterSeconds: number; run: () => void }>) {
  let elapsed = 0
  for (const step of steps) {
    elapsed += step.afterSeconds
    timers.setTimeout(step.run, elapsed * 1000)
  }
}
```

### State (SET_STATE / ON_STATE_CHANGE / WHEN_STATE_IS)

A smart item's "states" list is an enum on the entity. Use a custom component so the value
travels with the entity and shows up in the ECS, plus a plain callback list for the
change event.

```ts
import { engine, Schemas, type Entity } from '@dcl/sdk/ecs'

const DoorState = engine.defineComponent('scene::DoorState', {
  value: Schemas.EnumString<'closed' | 'opening' | 'open'>(
    { closed: 'closed', opening: 'opening', open: 'open' },
    'closed'
  )
})

function setState(door: Entity, next: 'closed' | 'opening' | 'open') {
  const state = DoorState.getMutable(door)
  if (state.value === next) return
  state.value = next
  onStateChange(door, next)
}
```

For a single entity that never needs to be cloned or queried, a module-level `let` is
fine and cheaper. Use a component when several entities share the behavior, or when a
system needs to iterate `engine.getEntitiesWith(DoorState)`.

### Counter (SET/INCREMENT/DECREASE_COUNTER, ON_COUNTER_CHANGE, WHEN_COUNTER_*)

```ts
const Counter = engine.defineComponent('scene::Counter', { value: Schemas.Int })

function addCounter(entity: Entity, amount = 1) {
  const counter = Counter.getMutable(entity)
  counter.value += amount
  if (counter.value >= 3) unlock()          // WHEN_COUNTER_IS_GREATER_THAN
}
```

### Conditional trigger (trigger conditions)

The palette's conditions are an `if` at the top of the handler.

```ts
onClick(chest, 'Open', () => {
  if (State.getOrNull(chest)?.current !== 'locked') return  // WHEN_STATE_IS
  if (Counter.get(chest).value < 3) return                  // WHEN_COUNTER_IS_LESS_THAN (negated)
  open(chest)
})
```

`AND` is sequential guards or `&&`, `OR` is `||`. Distance conditions:

```ts
function distanceToPlayer(entity: Entity): number {
  const player = Transform.getOrNull(engine.PlayerEntity)
  if (!player) return Infinity
  return Vector3.distance(worldPosition(entity), player.position)
}
```

### Tween end (ON_TWEEN_END)

There is no callback. Poll `tweenSystem.tweenCompleted(entity)` in a system; it returns
true on the single frame the tween finishes.

```ts
import { engine, Tween, tweenSystem } from '@dcl/sdk/ecs'

function doorSystem() {
  for (const [door, state] of engine.getEntitiesWith(DoorState)) {
    if (state.value !== 'opening') continue
    if (!Tween.has(door) || !tweenSystem.tweenCompleted(door)) continue
    DoorState.getMutable(door).value = 'open'
  }
}
engine.addSystem(doorSystem)
```

### Fire once (debounce)

```ts
function onClickOnce(entity: Entity, cb: () => void) {
  pointerEventsSystem.onPointerDown(
    { entity, opts: { button: InputAction.IA_POINTER, hoverText: 'Use' } },
    () => {
      pointerEventsSystem.removeOnPointerDown(entity)
      cb()
    }
  )
}
```

**PITFALL:** removing the handler from inside the handler is safe; *re-registering* one
from inside its own callback makes the same click fire several times. See the
`onPointerDown` pitfall in [[add-interactivity]].

### World position of a parented entity

Several actions (MOVE_PLAYER_HERE, PLAYER_FACE_ITEM, distance conditions) need scene-space
position, and `Transform.position` is parent-relative. Creator Hub scenes nest entities, so
this matters more when porting than when authoring from scratch.

```ts
export function worldPosition(entity: Entity): Vector3 {
  const t = Transform.getOrNull(entity)
  if (!t) return Vector3.Zero()
  if (!t.parent) return t.position
  return Vector3.add(worldPosition(t.parent), Vector3.rotate(t.position, worldRotation(t.parent)))
}

export function worldRotation(entity: Entity): Quaternion {
  const t = Transform.getOrNull(entity)
  if (!t) return Quaternion.Identity()
  if (!t.parent) return t.rotation
  return Quaternion.multiply(t.rotation, worldRotation(t.parent))
}
```

## scene.json permissions

Getting this wrong produces code that compiles and then silently does nothing.

```json
"requiredPermissions": ["ALLOW_TO_MOVE_PLAYER_INSIDE_SCENE", "ALLOW_TO_TRIGGER_AVATAR_EMOTE"]
```

| Permission | Gates | Actions |
| --- | --- | --- |
| `ALLOW_TO_MOVE_PLAYER_INSIDE_SCENE` | `movePlayerTo` | MOVE_PLAYER, MOVE_PLAYER_HERE, PLAYER_FACE_ITEM |
| `ALLOW_TO_TRIGGER_AVATAR_EMOTE` | `triggerEmote`, `triggerSceneEmote` | PLAY_DEFAULT_EMOTE, PLAY_CUSTOM_EMOTE |
| `OPEN_EXTERNAL_LINK` | `openExternalUrl` | OPEN_LINK |
| `USE_FETCH` | `fetch`, `signedFetch` | CLAIM_AIRDROP, any HTTP |
| `USE_WEBSOCKET` | `WebSocket` | — |
| `USE_WEB3_API` | wallet signing | — |
| `ALLOW_MEDIA_HOSTNAMES` | external media hosts (legacy; pair with `allowedMediaHostnames`) | PLAY_VIDEO_STREAM, PLAY_AUDIO_STREAM |

The full set of valid values is exactly those seven — the `RequiredPermission` union in
`@dcl/schemas`.

Two honest caveats:

- Permissions are enforced for **portable experiences and smart wearables**. Normal parcel
  and World scenes are not currently blocked by their absence. Declare them anyway: the
  smart items do, they document intent, and enforcement has changed before.
- Permissions are not the only gate. These are **restricted actions**, gated by the client
  regardless of `scene.json` (see [[scene-runtime]]):
  - `openExternalUrl` — must be called from an explicit click/button handler, and the
    player sees a confirmation screen. Calling it from a timer or a system is dropped.
  - `teleportTo`, `changeRealm` — player sees a confirmation screen. No permission needed.
  - `movePlayerTo`, `triggerEmote`, `triggerSceneEmote` — only work while the player is
    already inside the scene bounds, and `movePlayerTo` only to a destination inside the
    scene bounds.

## Actions with no SDK equivalent

Say so rather than inventing an API. See the reference entries for details.

- **CALL_SCRIPT_METHOD** — Creator Hub build-time machinery (`~sdk/script-utils`, a virtual
  module populated by the editor). In code, calling a function is the equivalent. To keep
  writing Script components instead, see [[script-components]].
- **CLAIM_AIRDROP** — the Decentraland Rewards HTTP service plus a captcha UI, not an SDK
  feature. Reachable with `signedFetch`, needs `USE_FETCH`.
- **SPAWN_ENTITY** — instantiates an editor composite. `Composite.instance` exists but is
  marked deprecated/unsupported. Write a factory function instead.
- **CLONE_ENTITY** — no SDK clone. A generic deep clone over `engine.componentsIter()` is
  possible (the reference gives one) but a factory function is usually the better answer.
- **DAMAGE / HEAL_PLAYER** (and ON_DAMAGE / ON_HEAL_PLAYER) — no health model in the SDK.
  Scene-owned state plus a proximity query.
- **SHOW_IMAGE / HIDE_IMAGE / SHOW_TEXT / HIDE_TEXT** — no imperative "show a toast" API.
  Use `@dcl/sdk/react-ecs`, one renderer for the whole scene driven by module state
  ([[build-ui]]).

## Cross-References

- [[add-interactivity]] — `pointerEventsSystem`, `TriggerArea`, raycasts: the mechanisms
  behind ON_CLICK, ON_INPUT_ACTION and the area triggers
- [[advanced-input]] — `inputSystem` polling (global triggers) and `InputModifier`
  (FREEZE_PLAYER)
- [[animations-tweens]] — `Animator`, `Tween`, `TweenSequence` (PLAY_ANIMATION, START_TWEEN,
  SLIDE_TEXTURE, ON_TWEEN_END)
- [[audio-video]] — `AudioSource`, `AudioStream`, `VideoPlayer` (PLAY_SOUND, PLAY_*_STREAM)
- [[camera-control]] — `VirtualCamera` / `MainCamera` (CHANGE_CAMERA)
- [[lighting-environment]] — `LightSource`, `SkyboxTime` (LIGHTS_*, CHANGE_SKYBOX)
- [[player-avatar]] — `movePlayerTo`, emotes, `AvatarAttach` (MOVE_PLAYER*, PLAY_*_EMOTE,
  ATTACH_TO_PLAYER)
- [[build-ui]] — React-ECS overlays (SHOW_TEXT / SHOW_IMAGE / ON_CLICK_IMAGE)
- [[scene-runtime]] — `timers`, restricted actions, system priority
- [[composites]] — the `asset-packs::*` components this skill removes, and the edit-mode
  rules for touching a Creator Hub composite at all
- [[script-components]] — the other way to write a smart item: a class on a Script component
- [[migrate-sdk6-to-sdk7]] — for an SDK6 scene, port to SDK7 first; this skill covers the
  smart items on top

## References

- `{baseDir}/references/actions.md` — all 63 `ActionType` values with SDK7 equivalents
- `{baseDir}/references/triggers.md` — all 19 `TriggerType` and 9 `TriggerConditionType`
  values with SDK7 equivalents
