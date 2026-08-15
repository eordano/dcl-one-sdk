import { GrowOnlyValueSetComponentDefinition, LastWriteWinElementSetComponentDefinition } from '../engine/component';
import { IEngine } from '../engine/types';
import { AnimatorComponentDefinitionExtended } from './extended/Animator';
import { AudioSourceComponentDefinitionExtended } from './extended/AudioSource';
import { AudioAnalysisComponentDefinitionExtended } from './extended/AudioAnalysis';
import type { AudioAnalysisView } from './extended/AudioAnalysis';
import { MaterialComponentDefinitionExtended } from './extended/Material';
import { MeshColliderComponentDefinitionExtended } from './extended/MeshCollider';
import { MeshRendererComponentDefinitionExtended } from './extended/MeshRenderer';
import { TweenComponentDefinitionExtended } from './extended/Tween';
import { LwwComponentGetter, GSetComponentGetter } from './generated/index.gen';
import { NameType } from './manual/Name';
import { ISyncComponentsType } from './manual/SyncComponents';
import { INetowrkEntityType } from './manual/NetworkEntity';
import { INetowrkParentType } from './manual/NetworkParent';
import { TransformComponentExtended } from './manual/Transform';
import { AudioStreamComponentDefinitionExtended } from './extended/AudioStream';
import { MediaState } from './generated/pb/decentraland/sdk/components/common/media_state.gen';
import { VirtualCameraComponentDefinitionExtended } from './extended/VirtualCamera';
import { InputModifierComponentDefinitionExtended } from './extended/InputModifier';
import { LightSourceComponentDefinitionExtended } from './extended/LightSource';
import { TriggerAreaComponentDefinitionExtended } from './extended/TriggerArea';
import { ParticleSystemComponentDefinitionExtended } from './extended/ParticleSystem';
import { TagsComponentDefinitionExtended } from './manual/Tags';
export * from './generated/index.gen';
export type { GrowOnlyValueSetComponentDefinition, LastWriteWinElementSetComponentDefinition, LwwComponentGetter, GSetComponentGetter };
export declare const Transform: LwwComponentGetter<TransformComponentExtended>;
export declare const Material: LwwComponentGetter<MaterialComponentDefinitionExtended>;
export declare const Animator: LwwComponentGetter<AnimatorComponentDefinitionExtended>;
export declare const AudioSource: LwwComponentGetter<AudioSourceComponentDefinitionExtended>;
export declare const AudioAnalysis: LwwComponentGetter<AudioAnalysisComponentDefinitionExtended>;
export declare const AudioStream: (engine: Pick<IEngine, 'defineComponentFromSchema' | 'defineValueSetComponentFromSchema'>) => AudioStreamComponentDefinitionExtended;
export declare const MeshRenderer: LwwComponentGetter<MeshRendererComponentDefinitionExtended>;
export declare const MeshCollider: LwwComponentGetter<MeshColliderComponentDefinitionExtended>;
export declare const Tween: LwwComponentGetter<TweenComponentDefinitionExtended>;
export declare const VirtualCamera: LwwComponentGetter<VirtualCameraComponentDefinitionExtended>;
export declare const InputModifier: LwwComponentGetter<InputModifierComponentDefinitionExtended>;
export declare const LightSource: LwwComponentGetter<LightSourceComponentDefinitionExtended>;
export declare const TriggerArea: LwwComponentGetter<TriggerAreaComponentDefinitionExtended>;
export declare const ParticleSystem: LwwComponentGetter<ParticleSystemComponentDefinitionExtended>;
/**
 * @alpha
 */
export declare const Name: (engine: Pick<IEngine, 'defineComponent'>) => LastWriteWinElementSetComponentDefinition<NameType>;
export declare const Tags: (engine: Pick<IEngine, 'defineComponent'>) => TagsComponentDefinitionExtended;
/**
 * @alpha
 */
export declare const SyncComponents: (engine: Pick<IEngine, 'defineComponent'>) => LastWriteWinElementSetComponentDefinition<ISyncComponentsType>;
/**
 * @alpha
 */
export declare const NetworkEntity: (engine: Pick<IEngine, 'defineComponent'>) => LastWriteWinElementSetComponentDefinition<INetowrkEntityType>;
/**
 * @alpha
 */
export declare const NetworkParent: (engine: Pick<IEngine, 'defineComponent'>) => LastWriteWinElementSetComponentDefinition<INetowrkParentType>;
export { MediaState };
export type { AudioAnalysisView };
