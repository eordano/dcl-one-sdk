# Smart-Item Actions → SDK7

All 63 `ActionType` values from `@dcl/asset-packs` (verified — `dist/enums.d.ts` in
`@dcl/asset-packs` 2.17.2 declares exactly 63), with the SDK7 they compile down to. Every
snippet below type-checks against `@dcl/sdk` 7.25.

For the porting method, the composition patterns (delay, loop, state, counter, world
position) and the `scene.json` permission table, see `{baseDir}/SKILL.md`. For the trigger
half of a smart item see `{baseDir}/references/triggers.md`.

Imports are elided per snippet; everything comes from `@dcl/sdk/ecs`, `@dcl/sdk/math`,
`@dcl/sdk/players`, or `~system/RestrictedActions` as noted.

Shared conventions:

- `entity: Entity` is the target the smart item was attached to.
- Durations in the palette are **seconds**; SDK tween and timer durations are
  **milliseconds**. Multiply by 1000.
- `getMutable` throws when the component is absent; `getMutableOrNull` returns null.

Each section names the topic skill that owns the component in full. This reference gives
the minimum correct call for one palette entry, not the complete API — load the named
skill when the port needs more than the smart item exposed.

## Index

| Action | Section |
| --- | --- |
| ATTACH_TO_PLAYER, DETACH_FROM_PLAYER | [Player attachment](#player-attachment) |
| BATCH, RANDOM | [Control flow](#control-flow) |
| CALL_SCRIPT_METHOD | [No SDK equivalent](#no-sdk-equivalent) |
| CHANGE_CAMERA | [Camera](#camera) |
| CHANGE_COLLISIONS | [Visibility and collision](#visibility-and-collision) |
| CHANGE_SKYBOX, RESET_SKYBOX | [Skybox](#skybox) |
| CHANGE_TEXT | [Text](#text) |
| CLAIM_AIRDROP | [No SDK equivalent](#no-sdk-equivalent) |
| CLONE_ENTITY, SPAWN_ENTITY | [Creating entities](#creating-entities) |
| DAMAGE, HEAL_PLAYER | [Damage and health](#damage-and-health) |
| DECREASE_COUNTER, INCREMENT_COUNTER, SET_COUNTER | [Counters and states](#counters-and-states) |
| DELETE, REMOVE_ENTITY | [Removing entities](#removing-entities) |
| FOLLOW_PLAYER, STOP_FOLLOWING_PLAYER | [Follow](#follow) |
| FREEZE_PLAYER, UNFREEZE_PLAYER | [Player input](#player-input) |
| HIDE_IMAGE, HIDE_TEXT, SHOW_IMAGE, SHOW_TEXT | [Screen UI](#screen-ui) |
| LIGHTS_MODIFY, LIGHTS_OFF, LIGHTS_ON | [Lights](#lights) |
| LOG_TO_CONSOLE | [Console](#console) |
| MOVE_PLAYER, MOVE_PLAYER_HERE, PLAYER_FACE_ITEM, TELEPORT_PLAYER | [Moving the player](#moving-the-player) |
| OPEN_LINK | [External links](#external-links) |
| PLACE_ON_CAMERA, PLACE_ON_PLAYER, ROTATE_AS_CAMERA, ROTATE_AS_PLAYER | [Snap to player or camera](#snap-to-player-or-camera) |
| PLAY_ANIMATION, STOP_ANIMATION | [Animation](#animation) |
| PLAY_AUDIO_STREAM, PLAY_SOUND, STOP_AUDIO_STREAM, STOP_SOUND | [Audio](#audio) |
| PLAY_CUSTOM_EMOTE, PLAY_DEFAULT_EMOTE | [Emotes](#emotes) |
| PLAY_VIDEO_STREAM, STOP_VIDEO_STREAM | [Video](#video) |
| SET_POSITION, SET_ROTATION, SET_SCALE | [Transform](#transform) |
| SET_STATE | [Counters and states](#counters-and-states) |
| SET_VISIBILITY | [Visibility and collision](#visibility-and-collision) |
| SLIDE_TEXTURE, START_TWEEN, STOP_TWEEN | [Tweens](#tweens) |
| START_DELAY, START_LOOP, STOP_DELAY, STOP_LOOP | [Timers](#timers) |

---

## Animation

Full component: [[animations-tweens]]. Requires a `GltfContainer` whose glb contains the
named clips.

### PLAY_ANIMATION

```ts
function playAnimation(entity: Entity, clip: string, loop = false) {
  if (!Animator.has(entity)) Animator.create(entity, { states: [{ clip }] })
  const animator = Animator.getMutable(entity)
  if (!animator.states.some((s) => s.clip === clip)) {
    animator.states = [...animator.states, { clip }]
  }
  Animator.playSingleAnimation(entity, clip, true)   // stops the others, resets cursor
  Animator.getClip(entity, clip).loop = loop
}
```

Declaring the clips up front is cleaner when you know them:

```ts
Animator.create(entity, {
  states: [
    { clip: 'Idle', playing: true, loop: true, weight: 1 },
    { clip: 'Open', playing: false, loop: false, weight: 1, speed: 1.2 }
  ]
})
Animator.playSingleAnimation(entity, 'Open')
```

`Animator.getClip` throws if the clip is not in `states`; `getClipOrNull` does not.

### STOP_ANIMATION

```ts
function stopAnimation(entity: Entity) {
  if (Animator.has(entity)) Animator.stopAllAnimations(entity, true)  // true = reset cursor
}
```

---

## Audio

Full component: [[audio-video]].

### PLAY_SOUND

Positional audio from a file in the scene folder.

```ts
function playSound(entity: Entity, src: string, loop = false, volume = 1) {
  if (AudioSource.has(entity)) {
    const audio = AudioSource.getMutable(entity)
    audio.audioClipUrl = src
    audio.loop = loop
    audio.volume = volume
    audio.playing = true
  } else {
    AudioSource.create(entity, { audioClipUrl: src, playing: true, loop, volume, global: false })
  }
}
```

To retrigger a sound that is already playing (a click sound on repeat clicks), use the
helper — flipping `playing` back to `true` on an already-true value is a no-op:

```ts
AudioSource.playSound(entity, src, true)   // third arg resets the cursor
```

`global: true` makes it non-positional (same volume everywhere in the scene).

### STOP_SOUND

```ts
function stopSound(entity: Entity) {
  const audio = AudioSource.getMutableOrNull(entity)
  if (audio) audio.playing = false
}
```

`AudioSource.stopSound(entity)` is equivalent.

### PLAY_AUDIO_STREAM

A live stream from a URL (icecast/shoutcast/HLS audio). Different component from
`AudioSource`; not positional.

```ts
function playAudioStream(entity: Entity, url: string, volume = 1) {
  AudioStream.createOrReplace(entity, { url, playing: true, volume })
}
```

External hosts: see `ALLOW_MEDIA_HOSTNAMES` in `{baseDir}/SKILL.md` → scene.json
permissions, and the media-hostname notes in [[audio-video]].

### STOP_AUDIO_STREAM

```ts
function stopAudioStream(entity: Entity) {
  const stream = AudioStream.getMutableOrNull(entity)
  if (stream) stream.playing = false
}
```

---

## Video

Full component: [[audio-video]].

### PLAY_VIDEO_STREAM

`VideoPlayer` produces a texture; something has to display it. The smart item silently
rewrites the entity's material, which is why "play video" on a plain cube appears to do
nothing until you do the same.

```ts
function playVideo(screen: Entity, src: string, volume = 1, loop = false) {
  VideoPlayer.createOrReplace(screen, { src, playing: true, volume, loop })
  Material.setBasicMaterial(screen, {
    texture: Material.Texture.Video({ videoPlayerEntity: screen })
  })
}
```

`src` is either a file in the scene folder (`videos/clip.mp4`) or an m3u8/live URL.
Use `setBasicMaterial` (unlit) so the video is not darkened by scene lighting.

React to buffering/ready:

```ts
videoEventsSystem.registerVideoEventsEntity(screen, (event) => {
  if (event.state === VideoState.VS_READY) console.log('ready')
})
```

### STOP_VIDEO_STREAM

```ts
function stopVideo(screen: Entity) {
  const video = VideoPlayer.getMutableOrNull(screen)
  if (video) video.playing = false
}
```

---

## Visibility and collision

Collider layers in full: [[add-3d-models]] (glb masks) and [[add-interactivity]]
(`CL_POINTER` and clickability).

### SET_VISIBILITY

```ts
VisibilityComponent.createOrReplace(entity, { visible: false })
```

Hiding does not remove colliders. The smart item optionally clears them too; do both
explicitly if you mean "gone":

```ts
function setVisibility(entity: Entity, visible: boolean) {
  VisibilityComponent.createOrReplace(entity, { visible })
  const collider = MeshCollider.getMutableOrNull(entity)
  if (collider) collider.collisionMask = visible ? ColliderLayer.CL_POINTER : ColliderLayer.CL_NONE
}
```

### CHANGE_COLLISIONS

```ts
// primitive shapes
MeshCollider.setBox(entity, [ColliderLayer.CL_POINTER, ColliderLayer.CL_PHYSICS])
const collider = MeshCollider.getMutableOrNull(entity)
if (collider) collider.collisionMask = ColliderLayer.CL_NONE

// glb models: two independent masks
const gltf = GltfContainer.getMutableOrNull(entity)
if (gltf) {
  gltf.visibleMeshesCollisionMask = ColliderLayer.CL_POINTER | ColliderLayer.CL_PHYSICS
  gltf.invisibleMeshesCollisionMask = ColliderLayer.CL_NONE
}
```

`CL_POINTER` = clickable, `CL_PHYSICS` = blocks the avatar, `CL_NONE` = neither. Also
`CL_PLAYER` / `CL_MAIN_PLAYER` (avatar layers, used by trigger areas) and
`CL_CUSTOM1`..`CL_CUSTOM8`.

In a glb, meshes whose object name ends in `_collider` are the invisible collider geometry
and are governed by `invisibleMeshesCollisionMask` (default: physics + pointer); everything
else is governed by `visibleMeshesCollisionMask` (default: no collision). Do not assign the
same layer to both masks.

---

## Text

Full component: [[advanced-rendering]] (`TextShape`, `Billboard`).

### CHANGE_TEXT

World-space 3D text.

```ts
function changeText(entity: Entity, text: string, fontSize?: number, color?: Color4) {
  const shape = TextShape.getMutableOrNull(entity)
  if (!shape) {
    TextShape.create(entity, { text, fontSize, textColor: color })
    return
  }
  shape.text = text
  if (fontSize !== undefined) shape.fontSize = fontSize
  if (color) shape.textColor = color
}
```

A label that always faces the player:

```ts
TextShape.create(label, {
  text, fontSize: 3, font: Font.F_SANS_SERIF,
  textAlign: TextAlignMode.TAM_MIDDLE_CENTER,
  textColor: Color4.White(), outlineWidth: 0.1, outlineColor: Color3.Black()
})
Billboard.create(label, { billboardMode: BillboardMode.BM_Y })
```

For screen-space text see [Screen UI](#screen-ui).

---

## Lights

Full component: [[lighting-environment]]. Requires a `LightSource` on the entity.

### LIGHTS_ON / LIGHTS_OFF

```ts
function setLight(entity: Entity, active: boolean) {
  const light = LightSource.getMutableOrNull(entity)
  if (light) light.active = active
}
```

### LIGHTS_MODIFY

```ts
function lightsModify(entity: Entity, color: Color3, intensity: number) {
  const light = LightSource.getMutableOrNull(entity)
  if (!light) return
  light.color = color
  light.intensity = intensity
}
```

Creating one:

```ts
LightSource.create(entity, {
  active: true,
  color: Color3.White(),
  intensity: 16000,          // candela; default 16000
  range: 12,                 // metres; -1 = derive from intensity
  shadow: true,
  type: LightSource.Type.Spot({ innerAngle: 20, outerAngle: 40 })
})
// or LightSource.Type.Point({})
```

---

## Skybox

Full component: [[lighting-environment]] (`SkyboxTime`, day/night cycle).

### CHANGE_SKYBOX

Time of day in seconds since midnight, 0..86400. Component lives on the root entity.

```ts
SkyboxTime.createOrReplace(engine.RootEntity, {
  fixedTime: 43200,                          // 12:00
  transitionMode: TransitionMode.TM_FORWARD
})
```

### RESET_SKYBOX

```ts
if (SkyboxTime.has(engine.RootEntity)) SkyboxTime.deleteFrom(engine.RootEntity)
```

---

## Camera

Full component: [[camera-control]] (`VirtualCamera`, `MainCamera`, `CameraModeArea`,
transitions).

### CHANGE_CAMERA

`VirtualCamera` marks a camera; `MainCamera` on `engine.CameraEntity` selects which one is
active. Deleting `MainCamera` returns control to the player.

```ts
function makeVirtualCamera(lookAt?: Entity): Entity {
  const cam = engine.addEntity()
  Transform.create(cam, { position: Vector3.create(8, 5, 8) })
  VirtualCamera.create(cam, {
    defaultTransition: { transitionMode: VirtualCamera.Transition.Time(1) },  // or .Speed(n)
    lookAtEntity: lookAt
  })
  return cam
}

MainCamera.createOrReplace(engine.CameraEntity, { virtualCameraEntity: cam })

// back to the player camera
if (MainCamera.has(engine.CameraEntity)) MainCamera.deleteFrom(engine.CameraEntity)
```

---

## Tweens

Full component: [[animations-tweens]] (`Tween`, `TweenSequence`, easing catalogue).
`Tween` interpolates a Transform on the client, without a per-frame system. Durations are
milliseconds. Setting a new `Tween` replaces the running one.

### START_TWEEN — move / rotate / scale

```ts
Tween.setMove(entity, Transform.get(entity).position, target, 2000, EasingFunction.EF_EASEOUTQUAD)
Tween.setRotate(entity, Transform.get(entity).rotation, Quaternion.fromEulerDegrees(0, 90, 0), 900)
Tween.setScale(entity, Transform.get(entity).scale, Vector3.create(2, 2, 2), 1000, EasingFunction.EF_EASEBOUNCE)
```

"Relative" in the palette means add to the current value:

```ts
const from = Transform.get(entity).position
Tween.setMove(entity, from, Vector3.add(from, offset), 2000)
```

All three at once:

```ts
Tween.setMoveRotateScale(entity, {
  position: { start: a, end: b },
  rotation: { start: Quaternion.Identity(), end: Quaternion.fromEulerDegrees(0, 180, 0) },
  scale: { start: Vector3.One(), end: Vector3.create(2, 2, 2) },
  duration: 1500,
  easingFunction: EasingFunction.EF_EASEOUTCUBIC
})
```

Easing names are `EasingFunction.EF_*`: `EF_LINEAR`, and in/out/inout variants of `QUAD`,
`SINE`, `EXPO`, `ELASTIC`, `BOUNCE`, `CUBIC`, `QUART`, `QUINT`, `CIRC`, `BACK`.

### START_TWEEN — keep rotating

```ts
Tween.setRotateContinuous(entity, Quaternion.fromEulerDegrees(0, 1, 0), 45, 0)
// axis as a quaternion, degrees/second, 0 duration = forever
```

`setMoveContinuous(entity, direction, speed, duration)` is the translation equivalent.

### Looping and chaining tweens

`TweenSequence` on the same entity. `TL_YOYO` plays the `Tween` forwards then backwards
forever; `TL_RESTART` repeats from the start.

```ts
Tween.setMove(entity, a, b, 2000, EasingFunction.EF_EASESINE)
TweenSequence.createOrReplace(entity, { sequence: [], loop: TweenLoop.TL_YOYO })
```

A → B → C then stop:

```ts
Tween.setMove(entity, a, b, 1000)
TweenSequence.createOrReplace(entity, {
  sequence: [
    { duration: 1000, easingFunction: EasingFunction.EF_LINEAR, mode: Tween.Mode.Move({ start: b, end: c }) }
  ]
})
```

### STOP_TWEEN

```ts
if (Tween.has(entity)) Tween.deleteFrom(entity)
if (TweenSequence.has(entity)) TweenSequence.deleteFrom(entity)
```

The entity stops where it is. To pause instead, set `Tween.getMutable(entity).playing = false`.

### SLIDE_TEXTURE

Scrolls the material's UVs. Also a Tween mode.

```ts
Tween.setTextureMoveContinuous(entity, { x: 0.1, y: 0 }, 1, TextureMovementType.TMT_OFFSET, 0)
```

`TMT_OFFSET` slides the texture; `TMT_TILING` scales it.

---

## Transform

`Transform.position` is relative to the parent. See `worldPosition()` in
`{baseDir}/SKILL.md` → World position of a parented entity for scene-space.

### SET_POSITION / SET_ROTATION / SET_SCALE

```ts
function setPosition(entity: Entity, p: Vector3, relative = false) {
  const t = Transform.getMutable(entity)
  t.position = relative ? Vector3.add(t.position, p) : p
}

function setRotation(entity: Entity, euler: Vector3, relative = false) {
  const t = Transform.getMutable(entity)
  const q = Quaternion.fromEulerDegrees(euler.x, euler.y, euler.z)
  t.rotation = relative ? Quaternion.multiply(t.rotation, q) : q
}

function setScale(entity: Entity, s: Vector3, relative = false) {
  const t = Transform.getMutable(entity)
  t.scale = relative ? Vector3.add(t.scale, s) : s
}
```

---

## Snap to player or camera

`engine.PlayerEntity` and `engine.CameraEntity` carry a read-only `Transform` updated by
the client each frame. Copy the value; do not alias it.

### PLACE_ON_PLAYER / ROTATE_AS_PLAYER

```ts
function placeOnPlayer(entity: Entity) {
  const player = Transform.getOrNull(engine.PlayerEntity)
  if (player) Transform.getMutable(entity).position = { ...player.position }
}

function rotateAsPlayer(entity: Entity) {
  const player = Transform.getOrNull(engine.PlayerEntity)
  if (player) Transform.getMutable(entity).rotation = { ...player.rotation }
}
```

### PLACE_ON_CAMERA / ROTATE_AS_CAMERA

Identical with `engine.CameraEntity`.

These are one-shot snaps. For continuous attachment, parent instead:

```ts
Transform.createOrReplace(entity, { position: Vector3.create(0, 2, 0), parent: engine.PlayerEntity })
```

---

## Player attachment

Full component: [[player-avatar]] (`AvatarAttach`, anchor points, and the held-item vs
cosmetic-item decision — a smart item that "attaches to the player" is often better as
`Transform.parent = engine.CameraEntity`).

### ATTACH_TO_PLAYER

Pins the entity to a bone of the avatar. The entity's own Transform is overridden — to
offset it, parent a child and transform the child.

```ts
AvatarAttach.createOrReplace(entity, { anchorPointId: AvatarAnchorPointType.AAPT_RIGHT_HAND })
```

Anchor points: `AAPT_NAME_TAG`, `AAPT_HEAD`, `AAPT_NECK`, `AAPT_SPINE`/`1`/`2`, `AAPT_HIP`,
`AAPT_LEFT_HAND`/`AAPT_RIGHT_HAND`, shoulder/arm/forearm/hand-index and leg/foot/toe
variants for both sides. `AAPT_POSITION` is deprecated — parent to `engine.PlayerEntity`
instead.

Attaching to a specific avatar (default is the local player):

```ts
const player = getPlayer()   // from '@dcl/sdk/players'
if (player) AvatarAttach.createOrReplace(entity, { avatarId: player.userId, anchorPointId: AvatarAnchorPointType.AAPT_NAME_TAG })
```

### DETACH_FROM_PLAYER

```ts
if (AvatarAttach.has(entity)) AvatarAttach.deleteFrom(entity)
```

---

## Player input

Full component: [[advanced-input]] (`InputModifier`, cutscene patterns).

### FREEZE_PLAYER / UNFREEZE_PLAYER

```ts
InputModifier.createOrReplace(engine.PlayerEntity, {
  mode: InputModifier.Mode.Standard({ disableAll: true })
})

InputModifier.createOrReplace(engine.PlayerEntity, {
  mode: InputModifier.Mode.Standard({ disableAll: false })
})
```

Finer than the palette allows: `disableWalk`, `disableJog`, `disableRun`, `disableJump`,
`disableDoubleJump`, `disableEmote`, `disableGliding`.

```ts
InputModifier.createOrReplace(engine.PlayerEntity, {
  mode: InputModifier.Mode.Standard({ disableJump: true, disableRun: true })
})
```

---

## Moving the player

All of these are restricted actions. See `{baseDir}/SKILL.md` → scene.json permissions,
[[player-avatar]] (`movePlayerTo`) and [[scene-runtime]] (restricted actions in general).

### MOVE_PLAYER

Needs `ALLOW_TO_MOVE_PLAYER_INSIDE_SCENE`. Only works while the player is inside the scene
bounds, and only to a destination inside the scene bounds. Position is scene-relative.

```ts
import { movePlayerTo } from '~system/RestrictedActions'

void movePlayerTo({
  newRelativePosition: Vector3.create(4, 0, 4),
  cameraTarget: Vector3.create(8, 1, 8),   // where the camera looks
  avatarTarget: Vector3.create(8, 1, 8),   // where the avatar faces
  duration: 1                              // seconds; omit for an instant jump
})
```

Awaitable: resolves `{ success: boolean }`, false if the player interrupts an
interpolated move.

### MOVE_PLAYER_HERE

Move the player to the item and face them the way the item faces.

```ts
function movePlayerHere(entity: Entity) {
  const here = worldPosition(entity)
  const forward = Vector3.rotate(Vector3.Forward(), worldRotation(entity))
  void movePlayerTo({ newRelativePosition: here, avatarTarget: Vector3.add(here, forward) })
}
```

### PLAYER_FACE_ITEM

Turn the player toward the item without moving them.

```ts
function playerFaceItem(entity: Entity) {
  const player = Transform.getOrNull(engine.PlayerEntity)
  if (!player) return
  void movePlayerTo({ newRelativePosition: player.position, avatarTarget: worldPosition(entity) })
}
```

### TELEPORT_PLAYER

Two different calls depending on the palette's `mode`. Both show the player a confirmation
screen. Neither needs a `scene.json` permission.

```ts
import { teleportTo, changeRealm } from '~system/RestrictedActions'

// to Genesis City parcel coordinates
void teleportTo({ worldCoordinates: { x: 72, y: -10 } })

// to a World
void changeRealm({ realm: 'my-world.dcl.eth', message: 'Jumping worlds' })
```

`changeRealm` back to Genesis City takes the realm provider URL as `realm`.

---

## Emotes

Full API: [[player-avatar]] (emotes, avatar masks). Both need
`ALLOW_TO_TRIGGER_AVATAR_EMOTE` and only fire while the player is inside the scene bounds.

### PLAY_DEFAULT_EMOTE

```ts
import { triggerEmote } from '~system/RestrictedActions'

void triggerEmote({ predefinedEmote: 'robot' })
void triggerEmote({ predefinedEmote: 'openChest', mask: AvatarMask.AM_UPPER_BODY })
```

Wheel emotes: `wave`, `fistpump`, `robot`, `raiseHand`, `clap`, `money`, `kiss`, `tik`,
`hammer`, `tektonik`, `dontsee`, `handsair`, `shrug`, `disco`, `dab`, `headexplode`.
Feedback emotes: `buttonDown`, `buttonFront`, `getHit`, `knockOut`, `lever`, `openChest`,
`openDoor`, `punch`, `push`, `swingWeaponOneHand`, `swingWeaponTwoHands`, `throw`,
`sittingChair1`, `sittingChair2`, `sittingGround1`, `sittingGround2`.

### PLAY_CUSTOM_EMOTE

```ts
import { triggerSceneEmote } from '~system/RestrictedActions'

void triggerSceneEmote({ src: 'animations/bow_emote.glb', loop: false })
```

The filename must end in `_emote.glb` or the client will not treat it as an avatar
animation. `stopEmote({})` cancels the current one.

---

## External links

### OPEN_LINK

```ts
import { openExternalUrl } from '~system/RestrictedActions'

void openExternalUrl({ url: 'https://decentraland.org' })
```

Must be called synchronously from a click/button handler. Calling it from a timer, a
system, or after an `await` is dropped by the client. The player sees a confirmation
screen naming the destination domain. `OPEN_EXTERNAL_LINK` in `requiredPermissions`. See
[[scene-runtime]] → restricted actions.

---

## Timers

Full API: [[scene-runtime]] (Timers).

### START_DELAY / STOP_DELAY

```ts
import { timers } from '@dcl/sdk/ecs'

const id = timers.setTimeout(() => closeDoor(), 3000)
timers.clearTimeout(id)
```

### START_LOOP / STOP_LOOP

```ts
const id = timers.setInterval(() => spawnWave(), 5000)
timers.clearInterval(id)
```

`timers` is bound to the scene engine and advances with scene time. The bare globals
`setTimeout`/`setInterval`/`clearTimeout`/`clearInterval` are the same implementation
polyfilled onto `globalThis`; either is fine, `timers.*` is clearer about which clock.

The palette's START_LOOP fires immediately and then every interval; `setInterval` waits one
interval first. Call the body once yourself if the difference matters.

---

## Counters and states

There is no SDK component for these; they are scene state. Define your own component so it
travels with the entity and can be queried by systems.

### SET_COUNTER / INCREMENT_COUNTER / DECREASE_COUNTER

```ts
// module level, not inside main()
export const Counter = engine.defineComponent('scene::Counter', { value: Schemas.Int })

function setCounter(entity: Entity, value: number) {
  Counter.getMutable(entity).value = value
  onCounterChange(entity, value)
}

function addCounter(entity: Entity, amount = 1) {
  setCounter(entity, Counter.get(entity).value + amount)
}

function subCounter(entity: Entity, amount = 1) {
  addCounter(entity, -amount)
}
```

The palette fires ON_COUNTER_CHANGE from all three; that is why the mutation goes through
one function. See `{baseDir}/references/triggers.md` → ON_COUNTER_CHANGE.

### SET_STATE

```ts
export const State = engine.defineComponent('scene::State', {
  current: Schemas.String,
  previous: Schemas.String
})

function setState(entity: Entity, next: string) {
  const state = State.getMutable(entity)
  if (state.current === next) return
  state.previous = state.current
  state.current = next
  onStateChange(entity, next, state.previous)
}
```

`Schemas.EnumString` gives you a typed state instead of a bare string:

```ts
const DoorState = engine.defineComponent('scene::DoorState', {
  value: Schemas.EnumString<'closed' | 'open'>({ closed: 'closed', open: 'open' }, 'closed')
})
```

---

## Control flow

### BATCH

```ts
function onActivate() {
  openDoor()
  playChime()
  addCounter(chest)
}
```

### RANDOM

```ts
function random(...actions: Array<() => void>) {
  if (actions.length === 0) return
  actions[Math.floor(Math.random() * actions.length)]()
}
```

---

## Follow

### FOLLOW_PLAYER / STOP_FOLLOWING_PLAYER

Needs a system; there is no follow component.

```ts
type FollowConfig = { speed: number; minDistance: number; axes: { x: boolean; y: boolean; z: boolean } }
const followers = new Map<Entity, FollowConfig>()

function followPlayer(entity: Entity, config: FollowConfig) { followers.set(entity, config) }
function stopFollowingPlayer(entity: Entity) { followers.delete(entity) }

function followSystem(dt: number) {
  const player = Transform.getOrNull(engine.PlayerEntity)
  if (!player) return
  for (const [entity, config] of followers) {
    const transform = Transform.getMutableOrNull(entity)
    if (!transform) continue
    const target = Vector3.create(
      config.axes.x ? player.position.x : transform.position.x,
      config.axes.y ? player.position.y : transform.position.y,
      config.axes.z ? player.position.z : transform.position.z
    )
    if (Vector3.distance(transform.position, target) <= config.minDistance) continue
    transform.position = Vector3.lerp(transform.position, target, Math.min(1, config.speed * dt))
  }
}
engine.addSystem(followSystem)
```

`dt` is seconds. Clamping the lerp factor at 1 keeps it stable on frame spikes.

---

## Creating entities

### SPAWN_ENTITY

The palette instantiates an editor-authored composite (see [[composites]] for that file
format). `Composite.instance` exists in `@dcl/ecs` but is marked deprecated and
unsupported. Write a factory:

```ts
function spawnCube(position: Vector3): Entity {
  const entity = engine.addEntity()
  Transform.create(entity, { position })
  MeshRenderer.setBox(entity)
  MeshCollider.setBox(entity, ColliderLayer.CL_POINTER)
  Material.setPbrMaterial(entity, { albedoColor: Color4.Teal() })
  return entity
}

function spawnModel(src: string, position: Vector3): Entity {
  const entity = engine.addEntity()
  Transform.create(entity, { position })
  GltfContainer.create(entity, { src })
  return entity
}
```

A factory is strictly better than a composite here: it takes parameters, and you attach the
spawned entity's behaviour in the same function (that is the palette's ON_SPAWN).

### CLONE_ENTITY

No SDK clone exists. Prefer a factory. When you genuinely must copy an unknown entity —
for instance one placed in the Creator Hub editor — iterate the engine's components:

```ts
function isLww(c: ComponentDefinition<unknown>): c is LastWriteWinElementSetComponentDefinition<unknown> {
  return 'createOrReplace' in c
}

function cloneEntity(source: Entity, position?: Vector3): Entity {
  const clone = engine.addEntity()
  for (const component of engine.componentsIter()) {
    if (!component.has(source) || !isLww(component)) continue
    component.createOrReplace(clone, JSON.parse(JSON.stringify(component.get(source))))
  }
  if (position) Transform.getMutable(clone).position = position
  return clone
}
```

With children, clone the tree and rewrite the parent links:

```ts
function cloneTree(root: Entity, position?: Vector3): Entity {
  const map = new Map<Entity, Entity>()
  for (const entity of treeOf(root)) map.set(entity, cloneEntity(entity))
  for (const clone of map.values()) {
    const transform = Transform.getMutableOrNull(clone)
    if (!transform?.parent) continue
    const parent = map.get(transform.parent)
    if (parent) transform.parent = parent
  }
  const clonedRoot = map.get(root)!
  if (position) Transform.getMutable(clonedRoot).position = position
  return clonedRoot
}

function treeOf(root: Entity): Entity[] {
  const out = [root]
  for (const [entity, transform] of engine.getEntitiesWith(Transform)) {
    if (transform.parent === root) out.push(...treeOf(entity))
  }
  return out
}
```

Only last-write-wins components can be copied this way; grow-only value-set components
(pointer results, video events) are engine-produced and must be skipped.

---

## Removing entities

### DELETE / REMOVE_ENTITY

Both palette entries do the same thing. Deleting a parent does not delete its children
unless you say so.

```ts
engine.removeEntityWithChildren(entity)   // entity and its Transform-parented descendants
engine.removeEntity(entity)               // just this one; children become orphans
```

Cancel any timers you armed for that entity first — a timer holding a stale `Entity` will
throw when it fires.

---

## Damage and health

### DAMAGE / HEAL_PLAYER

No health model exists in the SDK. Own it.

```ts
export const Health = engine.defineComponent('scene::Health', {
  current: Schemas.Number,
  max: Schemas.Number
})

function damageInRadius(origin: Entity, radius: number, hits = 1) {
  const center = worldPosition(origin)
  for (const [target] of engine.getEntitiesWith(Health, Transform)) {
    if (target === origin) continue
    if (Vector3.distance(center, worldPosition(target)) > radius) continue
    const health = Health.getMutable(target)
    health.current = Math.max(0, health.current - hits)
    onDamage(target, hits)
  }
}

function heal(target: Entity, amount = 1) {
  const health = Health.getMutable(target)
  health.current = Math.min(health.max, health.current + amount)
  onHeal(target, amount)
}
```

The palette's `layer` filter (player / non-player / all) becomes a check on whether the
target's Transform root is `engine.PlayerEntity`. There is no avatar health in
Decentraland; "damaging the player" only ever meant "decrement a number the scene owns and
draw it in the UI".

---

## Screen UI

Full API: [[build-ui]]. The palette's SHOW_TEXT / SHOW_IMAGE draw a screen-space overlay.
SDK7 does that with `@dcl/sdk/react-ecs`: one renderer for the whole scene, re-rendered
when module state changes. There is no imperative "show a toast" call.

```tsx
// src/ui.tsx
import { timers } from '@dcl/sdk/ecs'
import { Color4 } from '@dcl/sdk/math'
import ReactEcs, { ReactEcsRenderer, UiEntity, Label } from '@dcl/sdk/react-ecs'

const banner = { text: '', visible: false }
const popup = { src: '', visible: false }
let bannerTimer: number | undefined
let onImageClick: (() => void) | undefined

export function showText(text: string, hideAfterSeconds?: number) {      // SHOW_TEXT
  banner.text = text
  banner.visible = true
  if (bannerTimer !== undefined) timers.clearTimeout(bannerTimer)
  if (hideAfterSeconds) bannerTimer = timers.setTimeout(hideText, hideAfterSeconds * 1000)
}

export function hideText() { banner.visible = false }                     // HIDE_TEXT

export function showImage(src: string, hideAfterSeconds?: number, onClick?: () => void) {  // SHOW_IMAGE
  popup.src = src
  popup.visible = true
  onImageClick = onClick
  if (hideAfterSeconds) timers.setTimeout(hideImage, hideAfterSeconds * 1000)
}

export function hideImage() { popup.visible = false }                     // HIDE_IMAGE

const sceneUi = () => (
  <UiEntity
    uiTransform={{
      width: '100%', height: '100%', positionType: 'absolute',
      alignItems: 'center', justifyContent: 'center', flexDirection: 'column',
      pointerFilter: 'none'
    }}
  >
    {banner.visible && (
      <Label value={banner.text} fontSize={24} color={Color4.White()} uiTransform={{ height: 40 }} />
    )}
    {popup.visible && (
      <UiEntity
        uiTransform={{ width: 512, height: 320, pointerFilter: 'block' }}
        uiBackground={{ textureMode: 'stretch', texture: { src: popup.src } }}
        onMouseDown={() => onImageClick?.()}          // ON_CLICK_IMAGE
      />
    )}
  </UiEntity>
)

export function setupUi() { ReactEcsRenderer.setUiRenderer(sceneUi) }
```

Call `setupUi()` once from `main()`. `pointerFilter: 'none'` on the root is important —
a full-screen container that blocks pointer events swallows every click in the scene.
For a second, independent overlay (one smart item owning its own widget) use
`ReactEcsRenderer.addUiRenderer(ownerEntity, ...)` instead of a second `setUiRenderer` —
see [[build-ui]].

---

## Console

### LOG_TO_CONSOLE

```ts
console.log('message')
```

Only `console.log` and `console.error` exist in the scene runtime.

---

## No SDK equivalent

### CALL_SCRIPT_METHOD

Creator Hub machinery. The editor writes a `Script` component naming a file and a method,
and the build step generates a virtual module `~sdk/script-utils` that resolves the name to
a real function. Outside the editor, that module does not exist. To keep authoring Script
components rather than porting them out, see [[script-components]].

The honest translation is: import the module and call the function.

```ts
import { solve } from './padlock'
solve(entity)
```

If you are extending a Creator-Hub scene that already uses scripts, keep the script file
and call it directly rather than routing through the action dispatcher.

### CLAIM_AIRDROP

Not an SDK feature. `signedFetch` itself is documented in [[scene-runtime]]. This is an
HTTP conversation with the Decentraland Rewards service
(`https://rewards.decentraland.org`), optionally gated by a captcha the smart item renders
itself. Needs `USE_FETCH` in `requiredPermissions`, and `signedFetch` so the server can
authenticate the player.

```ts
import { signedFetch } from '~system/SignedFetch'
import { getRealm } from '~system/Runtime'
import { getPlayer } from '@dcl/sdk/players'

async function claimAirdrop(campaignKey: string): Promise<string | null> {
  const realm = await getRealm({})
  const player = getPlayer()
  const response = await signedFetch({
    url: 'https://rewards.decentraland.org/api/rewards',
    init: {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        campaign_key: campaignKey,
        beneficiary: player && !player.isGuest ? player.userId : '',
        catalyst: realm.realmInfo?.baseUrl ?? ''
      })
    }
  })
  return response.ok ? response.body : null
}
```

You still have to create the campaign in the Rewards dashboard and hold the dispenser key.
If the campaign requires a captcha you must build that UI yourself; the smart item's
captcha prompt is a react-ecs form inside `@dcl/asset-packs`, not something the SDK offers.
