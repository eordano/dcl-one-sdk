import { defineAnimatorComponent } from './extended/Animator';
import { defineAudioSourceComponent } from './extended/AudioSource';
import { defineAudioAnalysisComponent } from './extended/AudioAnalysis';
import { defineMaterialComponent } from './extended/Material';
import { defineMeshColliderComponent } from './extended/MeshCollider';
import { defineMeshRendererComponent } from './extended/MeshRenderer';
import { defineTweenComponent } from './extended/Tween';
import defineNameComponent from './manual/Name';
import defineSyncComponent from './manual/SyncComponents';
import defineNetworkEntity from './manual/NetworkEntity';
import defineNetworkParent from './manual/NetworkParent';
import { defineTransformComponent } from './manual/Transform';
import { defineAudioStreamComponent } from './extended/AudioStream';
import { MediaState } from './generated/pb/decentraland/sdk/components/common/media_state.gen';
import { defineVirtualCameraComponent } from './extended/VirtualCamera';
import { defineInputModifierComponent } from './extended/InputModifier';
import { defineLightSourceComponent } from './extended/LightSource';
import { defineTriggerAreaComponent } from './extended/TriggerArea';
import { defineParticleSystemComponent } from './extended/ParticleSystem';
import defineTagsComponent from './manual/Tags';
export * from './generated/index.gen';
/* @__PURE__ */
export const Transform = (engine) => defineTransformComponent(engine);
/* @__PURE__ */
export const Material = (engine) => defineMaterialComponent(engine);
/* @__PURE__ */
export const Animator = (engine) => defineAnimatorComponent(engine);
/* @__PURE__ */
export const AudioSource = (engine) => defineAudioSourceComponent(engine);
/* @__PURE__ */
export const AudioAnalysis = (engine) => defineAudioAnalysisComponent(engine);
/* @__PURE__ */
export const AudioStream = (engine) => defineAudioStreamComponent(engine);
/* @__PURE__ */
export const MeshRenderer = (engine) => defineMeshRendererComponent(engine);
/* @__PURE__ */
export const MeshCollider = (engine) => defineMeshColliderComponent(engine);
/* @__PURE__ */
export const Tween = (engine) => defineTweenComponent(engine);
/* @__PURE__ */
export const VirtualCamera = (engine) => defineVirtualCameraComponent(engine);
/* @__PURE__*/
export const InputModifier = (engine) => defineInputModifierComponent(engine);
/* @__PURE__ */
export const LightSource = (engine) => defineLightSourceComponent(engine);
/* @__PURE__ */
export const TriggerArea = (engine) => defineTriggerAreaComponent(engine);
/* @__PURE__ */
export const ParticleSystem = (engine) => defineParticleSystemComponent(engine);
/**
 * @alpha
 */
/* @__PURE__ */
export const Name = (engine) => defineNameComponent(engine);
/* @__PURE__ */
export const Tags = (engine) => defineTagsComponent(engine);
/**
 * @alpha
 */
/* @__PURE__ */
export const SyncComponents = (engine) => defineSyncComponent(engine);
/**
 * @alpha
 */
/* @__PURE__ */
export const NetworkEntity = (engine) => defineNetworkEntity(engine);
/**
 * @alpha
 */
/* @__PURE__ */
export const NetworkParent = (engine) => defineNetworkParent(engine);
export { MediaState };
