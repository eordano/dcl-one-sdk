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
var __setModuleDefault = (this && this.__setModuleDefault) || (Object.create ? (function(o, v) {
    Object.defineProperty(o, "default", { enumerable: true, value: v });
}) : function(o, v) {
    o["default"] = v;
});
var __importStar = (this && this.__importStar) || function (mod) {
    if (mod && mod.__esModule) return mod;
    var result = {};
    if (mod != null) for (var k in mod) if (k !== "default" && Object.prototype.hasOwnProperty.call(mod, k)) __createBinding(result, mod, k);
    __setModuleDefault(result, mod);
    return result;
};
var __exportStar = (this && this.__exportStar) || function(m, exports) {
    for (var p in m) if (p !== "default" && !Object.prototype.hasOwnProperty.call(exports, p)) __createBinding(exports, m, p);
};
Object.defineProperty(exports, "__esModule", { value: true });
exports.VideoEvent = exports.UiTransform = exports.UiText = exports.UiInputResult = exports.UiInput = exports.UiDropdownResult = exports.UiDropdown = exports.UiCanvasInformation = exports.UiBackground = exports.TweenState = exports.TweenSequence = exports.TriggerAreaResult = exports.TriggerArea = exports.TextShape = exports.SkyboxTime = exports.RealmInfo = exports.RaycastResult = exports.Raycast = exports.PrimaryPointerInfo = exports.PointerLock = exports.PointerEventsResult = exports.PointerEvents = exports.PlayerIdentityData = exports.PhysicsCombinedImpulse = exports.PhysicsCombinedForce = exports.ParticleSystem = exports.NftShape = exports.MainCamera = exports.LightSource = exports.InputModifier = exports.GltfNodeModifiers = exports.GltfContainerLoadingState = exports.GltfContainer = exports.EngineInfo = exports.CameraModeArea = exports.CameraMode = exports.Billboard = exports.AvatarShape = exports.AvatarModifierArea = exports.AvatarLocomotionSettings = exports.AvatarEquippedData = exports.AvatarEmoteCommand = exports.AvatarBase = exports.AvatarAttach = exports.AudioStream = exports.AudioSource = exports.AudioEvent = exports.AudioAnalysis = exports.AssetLoadLoadingState = exports.AssetLoad = void 0;
exports.VisibilityComponent = exports.VirtualCamera = exports.VideoPlayer = void 0;
const initialization_1 = require("../../runtime/initialization");
const components = __importStar(require("./index.gen"));
__exportStar(require("./index.gen"), exports);
/** @public */ exports.AssetLoad = components.AssetLoad(initialization_1.engine);
/** @public */ exports.AssetLoadLoadingState = components.AssetLoadLoadingState(initialization_1.engine);
/** @public */ exports.AudioAnalysis = components.AudioAnalysis(initialization_1.engine);
/** @public */ exports.AudioEvent = components.AudioEvent(initialization_1.engine);
/** @public */ exports.AudioSource = components.AudioSource(initialization_1.engine);
/** @public */ exports.AudioStream = components.AudioStream(initialization_1.engine);
/** @public */ exports.AvatarAttach = components.AvatarAttach(initialization_1.engine);
/** @public */ exports.AvatarBase = components.AvatarBase(initialization_1.engine);
/** @public */ exports.AvatarEmoteCommand = components.AvatarEmoteCommand(initialization_1.engine);
/** @public */ exports.AvatarEquippedData = components.AvatarEquippedData(initialization_1.engine);
/** @public */ exports.AvatarLocomotionSettings = components.AvatarLocomotionSettings(initialization_1.engine);
/** @public */ exports.AvatarModifierArea = components.AvatarModifierArea(initialization_1.engine);
/** @public */ exports.AvatarShape = components.AvatarShape(initialization_1.engine);
/** @public */ exports.Billboard = components.Billboard(initialization_1.engine);
/** @public */ exports.CameraMode = components.CameraMode(initialization_1.engine);
/** @public */ exports.CameraModeArea = components.CameraModeArea(initialization_1.engine);
/** @public */ exports.EngineInfo = components.EngineInfo(initialization_1.engine);
/** @public */ exports.GltfContainer = components.GltfContainer(initialization_1.engine);
/** @public */ exports.GltfContainerLoadingState = components.GltfContainerLoadingState(initialization_1.engine);
/** @public */ exports.GltfNodeModifiers = components.GltfNodeModifiers(initialization_1.engine);
/** @public */ exports.InputModifier = components.InputModifier(initialization_1.engine);
/** @public */ exports.LightSource = components.LightSource(initialization_1.engine);
/** @public */ exports.MainCamera = components.MainCamera(initialization_1.engine);
/** @public */ exports.NftShape = components.NftShape(initialization_1.engine);
/** @public */ exports.ParticleSystem = components.ParticleSystem(initialization_1.engine);
/** @public */ exports.PhysicsCombinedForce = components.PhysicsCombinedForce(initialization_1.engine);
/** @public */ exports.PhysicsCombinedImpulse = components.PhysicsCombinedImpulse(initialization_1.engine);
/** @public */ exports.PlayerIdentityData = components.PlayerIdentityData(initialization_1.engine);
/** @public */ exports.PointerEvents = components.PointerEvents(initialization_1.engine);
/** @public */ exports.PointerEventsResult = components.PointerEventsResult(initialization_1.engine);
/** @public */ exports.PointerLock = components.PointerLock(initialization_1.engine);
/** @public */ exports.PrimaryPointerInfo = components.PrimaryPointerInfo(initialization_1.engine);
/** @public */ exports.Raycast = components.Raycast(initialization_1.engine);
/** @public */ exports.RaycastResult = components.RaycastResult(initialization_1.engine);
/** @public */ exports.RealmInfo = components.RealmInfo(initialization_1.engine);
/** @public */ exports.SkyboxTime = components.SkyboxTime(initialization_1.engine);
/** @public */ exports.TextShape = components.TextShape(initialization_1.engine);
/** @public */ exports.TriggerArea = components.TriggerArea(initialization_1.engine);
/** @public */ exports.TriggerAreaResult = components.TriggerAreaResult(initialization_1.engine);
/** @public */ exports.TweenSequence = components.TweenSequence(initialization_1.engine);
/** @public */ exports.TweenState = components.TweenState(initialization_1.engine);
/** @public */ exports.UiBackground = components.UiBackground(initialization_1.engine);
/** @public */ exports.UiCanvasInformation = components.UiCanvasInformation(initialization_1.engine);
/** @public */ exports.UiDropdown = components.UiDropdown(initialization_1.engine);
/** @public */ exports.UiDropdownResult = components.UiDropdownResult(initialization_1.engine);
/** @public */ exports.UiInput = components.UiInput(initialization_1.engine);
/** @public */ exports.UiInputResult = components.UiInputResult(initialization_1.engine);
/** @public */ exports.UiText = components.UiText(initialization_1.engine);
/** @public */ exports.UiTransform = components.UiTransform(initialization_1.engine);
/** @public */ exports.VideoEvent = components.VideoEvent(initialization_1.engine);
/** @public */ exports.VideoPlayer = components.VideoPlayer(initialization_1.engine);
/** @public */ exports.VirtualCamera = components.VirtualCamera(initialization_1.engine);
/** @public */ exports.VisibilityComponent = components.VisibilityComponent(initialization_1.engine);
