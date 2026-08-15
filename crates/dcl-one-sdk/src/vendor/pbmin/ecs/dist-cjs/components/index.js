"use strict";
var __createBinding = (this && this.__createBinding) || (Object.create ? (function(o, m, k, k2) {
    if (k2 === undefined) k2 = k;
    var desc = Object.getOwnPropertyDescriptor(m, k);
    if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
      desc = { enumerable: true, get: function() { return m[k]; } };
    }
    Object.defineProperty(o, k2, desc);
}) : (function(o, m, k, k2) {
    if (k2 === undefined) k2 = k;
    o[k2] = m[k];
}));
var __exportStar = (this && this.__exportStar) || function(m, exports) {
    for (var p in m) if (p !== "default" && !Object.prototype.hasOwnProperty.call(exports, p)) __createBinding(exports, m, p);
};
var __importDefault = (this && this.__importDefault) || function (mod) {
    return (mod && mod.__esModule) ? mod : { "default": mod };
};
Object.defineProperty(exports, "__esModule", { value: true });
exports.MediaState = exports.NetworkParent = exports.NetworkEntity = exports.SyncComponents = exports.Tags = exports.Name = exports.ParticleSystem = exports.TriggerArea = exports.LightSource = exports.InputModifier = exports.VirtualCamera = exports.Tween = exports.MeshCollider = exports.MeshRenderer = exports.AudioStream = exports.AudioAnalysis = exports.AudioSource = exports.Animator = exports.Material = exports.Transform = void 0;
const Animator_1 = require("./extended/Animator");
const AudioSource_1 = require("./extended/AudioSource");
const AudioAnalysis_1 = require("./extended/AudioAnalysis");
const Material_1 = require("./extended/Material");
const MeshCollider_1 = require("./extended/MeshCollider");
const MeshRenderer_1 = require("./extended/MeshRenderer");
const Tween_1 = require("./extended/Tween");
const Name_1 = __importDefault(require("./manual/Name"));
const SyncComponents_1 = __importDefault(require("./manual/SyncComponents"));
const NetworkEntity_1 = __importDefault(require("./manual/NetworkEntity"));
const NetworkParent_1 = __importDefault(require("./manual/NetworkParent"));
const Transform_1 = require("./manual/Transform");
const AudioStream_1 = require("./extended/AudioStream");
const media_state_gen_1 = require("./generated/pb/decentraland/sdk/components/common/media_state.gen");
Object.defineProperty(exports, "MediaState", { enumerable: true, get: function () { return media_state_gen_1.MediaState; } });
const VirtualCamera_1 = require("./extended/VirtualCamera");
const InputModifier_1 = require("./extended/InputModifier");
const LightSource_1 = require("./extended/LightSource");
const TriggerArea_1 = require("./extended/TriggerArea");
const ParticleSystem_1 = require("./extended/ParticleSystem");
const Tags_1 = __importDefault(require("./manual/Tags"));
__exportStar(require("./generated/index.gen"), exports);
/* @__PURE__ */
const Transform = (engine) => (0, Transform_1.defineTransformComponent)(engine);
exports.Transform = Transform;
/* @__PURE__ */
const Material = (engine) => (0, Material_1.defineMaterialComponent)(engine);
exports.Material = Material;
/* @__PURE__ */
const Animator = (engine) => (0, Animator_1.defineAnimatorComponent)(engine);
exports.Animator = Animator;
/* @__PURE__ */
const AudioSource = (engine) => (0, AudioSource_1.defineAudioSourceComponent)(engine);
exports.AudioSource = AudioSource;
/* @__PURE__ */
const AudioAnalysis = (engine) => (0, AudioAnalysis_1.defineAudioAnalysisComponent)(engine);
exports.AudioAnalysis = AudioAnalysis;
/* @__PURE__ */
const AudioStream = (engine) => (0, AudioStream_1.defineAudioStreamComponent)(engine);
exports.AudioStream = AudioStream;
/* @__PURE__ */
const MeshRenderer = (engine) => (0, MeshRenderer_1.defineMeshRendererComponent)(engine);
exports.MeshRenderer = MeshRenderer;
/* @__PURE__ */
const MeshCollider = (engine) => (0, MeshCollider_1.defineMeshColliderComponent)(engine);
exports.MeshCollider = MeshCollider;
/* @__PURE__ */
const Tween = (engine) => (0, Tween_1.defineTweenComponent)(engine);
exports.Tween = Tween;
/* @__PURE__ */
const VirtualCamera = (engine) => (0, VirtualCamera_1.defineVirtualCameraComponent)(engine);
exports.VirtualCamera = VirtualCamera;
/* @__PURE__*/
const InputModifier = (engine) => (0, InputModifier_1.defineInputModifierComponent)(engine);
exports.InputModifier = InputModifier;
/* @__PURE__ */
const LightSource = (engine) => (0, LightSource_1.defineLightSourceComponent)(engine);
exports.LightSource = LightSource;
/* @__PURE__ */
const TriggerArea = (engine) => (0, TriggerArea_1.defineTriggerAreaComponent)(engine);
exports.TriggerArea = TriggerArea;
/* @__PURE__ */
const ParticleSystem = (engine) => (0, ParticleSystem_1.defineParticleSystemComponent)(engine);
exports.ParticleSystem = ParticleSystem;
/**
 * @alpha
 */
/* @__PURE__ */
const Name = (engine) => (0, Name_1.default)(engine);
exports.Name = Name;
/* @__PURE__ */
const Tags = (engine) => (0, Tags_1.default)(engine);
exports.Tags = Tags;
/**
 * @alpha
 */
/* @__PURE__ */
const SyncComponents = (engine) => (0, SyncComponents_1.default)(engine);
exports.SyncComponents = SyncComponents;
/**
 * @alpha
 */
/* @__PURE__ */
const NetworkEntity = (engine) => (0, NetworkEntity_1.default)(engine);
exports.NetworkEntity = NetworkEntity;
/**
 * @alpha
 */
/* @__PURE__ */
const NetworkParent = (engine) => (0, NetworkParent_1.default)(engine);
exports.NetworkParent = NetworkParent;
