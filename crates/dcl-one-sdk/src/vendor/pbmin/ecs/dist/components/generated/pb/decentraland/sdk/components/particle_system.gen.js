/* eslint-disable */
import _m0 from "protobufjs/minimal";
import { ColorRange } from "../../common/colors.gen";
import { FloatRange } from "../../common/floats.gen";
import { Texture } from "../../common/texture.gen";
import { Quaternion, Vector3 } from "../../common/vectors.gen";
const protobufPackageSarasa = "decentraland.sdk.components";
/**
 * @public
 */
export var PBParticleSystem_BlendMode;
(function (PBParticleSystem_BlendMode) {
    /** PSB_ALPHA - Standard alpha transparency. */
    PBParticleSystem_BlendMode[PBParticleSystem_BlendMode["PSB_ALPHA"] = 0] = "PSB_ALPHA";
    /** PSB_ADD - Additive blending (brightens underlying pixels). */
    PBParticleSystem_BlendMode[PBParticleSystem_BlendMode["PSB_ADD"] = 1] = "PSB_ADD";
    /** PSB_MULTIPLY - Multiply blending (darkens underlying pixels). */
    PBParticleSystem_BlendMode[PBParticleSystem_BlendMode["PSB_MULTIPLY"] = 2] = "PSB_MULTIPLY";
})(PBParticleSystem_BlendMode || (PBParticleSystem_BlendMode = {}));
/**
 * @public
 */
export var PBParticleSystem_PlaybackState;
(function (PBParticleSystem_PlaybackState) {
    /** PS_PLAYING - Particle system is emitting and simulating. */
    PBParticleSystem_PlaybackState[PBParticleSystem_PlaybackState["PS_PLAYING"] = 0] = "PS_PLAYING";
    /** PS_PAUSED - Simulation is frozen; no new particles are emitted. */
    PBParticleSystem_PlaybackState[PBParticleSystem_PlaybackState["PS_PAUSED"] = 1] = "PS_PAUSED";
    /** PS_STOPPED - Simulation stopped and existing particles cleared. */
    PBParticleSystem_PlaybackState[PBParticleSystem_PlaybackState["PS_STOPPED"] = 2] = "PS_STOPPED";
})(PBParticleSystem_PlaybackState || (PBParticleSystem_PlaybackState = {}));
/**
 * @public
 */
export var PBParticleSystem_SimulationSpace;
(function (PBParticleSystem_SimulationSpace) {
    /** PSS_LOCAL - Particles move with the entity transform. */
    PBParticleSystem_SimulationSpace[PBParticleSystem_SimulationSpace["PSS_LOCAL"] = 0] = "PSS_LOCAL";
    /** PSS_WORLD - Particles stay in world position after emission. */
    PBParticleSystem_SimulationSpace[PBParticleSystem_SimulationSpace["PSS_WORLD"] = 1] = "PSS_WORLD";
})(PBParticleSystem_SimulationSpace || (PBParticleSystem_SimulationSpace = {}));
function createBasePBParticleSystem() {
    return {
        active: undefined,
        rate: undefined,
        maxParticles: undefined,
        lifetime: undefined,
        gravity: undefined,
        additionalForce: undefined,
        initialSize: undefined,
        sizeOverTime: undefined,
        initialRotation: undefined,
        rotationOverTime: undefined,
        faceTravelDirection: undefined,
        initialColor: undefined,
        colorOverTime: undefined,
        initialVelocitySpeed: undefined,
        texture: undefined,
        blendMode: undefined,
        billboard: undefined,
        spriteSheet: undefined,
        shape: undefined,
        loop: undefined,
        prewarm: undefined,
        simulationSpace: undefined,
        limitVelocity: undefined,
        playbackState: undefined,
        bursts: undefined,
    };
}
/**
 * @public
 */
export var PBParticleSystem;
(function (PBParticleSystem) {
    function encode(message, writer = _m0.Writer.create()) {
        if (message.active !== undefined) {
            writer.uint32(8).bool(message.active);
        }
        if (message.rate !== undefined) {
            writer.uint32(21).float(message.rate);
        }
        if (message.maxParticles !== undefined) {
            writer.uint32(24).uint32(message.maxParticles);
        }
        if (message.lifetime !== undefined) {
            writer.uint32(37).float(message.lifetime);
        }
        if (message.gravity !== undefined) {
            writer.uint32(45).float(message.gravity);
        }
        if (message.additionalForce !== undefined) {
            Vector3.encode(message.additionalForce, writer.uint32(50).fork()).ldelim();
        }
        if (message.initialSize !== undefined) {
            FloatRange.encode(message.initialSize, writer.uint32(58).fork()).ldelim();
        }
        if (message.sizeOverTime !== undefined) {
            FloatRange.encode(message.sizeOverTime, writer.uint32(66).fork()).ldelim();
        }
        if (message.initialRotation !== undefined) {
            Quaternion.encode(message.initialRotation, writer.uint32(74).fork()).ldelim();
        }
        if (message.rotationOverTime !== undefined) {
            Quaternion.encode(message.rotationOverTime, writer.uint32(82).fork()).ldelim();
        }
        if (message.faceTravelDirection !== undefined) {
            writer.uint32(224).bool(message.faceTravelDirection);
        }
        if (message.initialColor !== undefined) {
            ColorRange.encode(message.initialColor, writer.uint32(90).fork()).ldelim();
        }
        if (message.colorOverTime !== undefined) {
            ColorRange.encode(message.colorOverTime, writer.uint32(98).fork()).ldelim();
        }
        if (message.initialVelocitySpeed !== undefined) {
            FloatRange.encode(message.initialVelocitySpeed, writer.uint32(106).fork()).ldelim();
        }
        if (message.texture !== undefined) {
            Texture.encode(message.texture, writer.uint32(114).fork()).ldelim();
        }
        if (message.blendMode !== undefined) {
            writer.uint32(120).int32(message.blendMode);
        }
        if (message.billboard !== undefined) {
            writer.uint32(128).bool(message.billboard);
        }
        if (message.spriteSheet !== undefined) {
            PBParticleSystem_SpriteSheetAnimation.encode(message.spriteSheet, writer.uint32(138).fork()).ldelim();
        }
        switch (message.shape?.$case) {
            case "point":
                PBParticleSystem_Point.encode(message.shape.point, writer.uint32(146).fork()).ldelim();
                break;
            case "sphere":
                PBParticleSystem_Sphere.encode(message.shape.sphere, writer.uint32(154).fork()).ldelim();
                break;
            case "cone":
                PBParticleSystem_Cone.encode(message.shape.cone, writer.uint32(162).fork()).ldelim();
                break;
            case "box":
                PBParticleSystem_Box.encode(message.shape.box, writer.uint32(170).fork()).ldelim();
                break;
        }
        if (message.loop !== undefined) {
            writer.uint32(192).bool(message.loop);
        }
        if (message.prewarm !== undefined) {
            writer.uint32(200).bool(message.prewarm);
        }
        if (message.simulationSpace !== undefined) {
            writer.uint32(216).int32(message.simulationSpace);
        }
        if (message.limitVelocity !== undefined) {
            PBParticleSystem_LimitVelocity.encode(message.limitVelocity, writer.uint32(210).fork()).ldelim();
        }
        if (message.playbackState !== undefined) {
            writer.uint32(176).int32(message.playbackState);
        }
        if (message.bursts !== undefined) {
            PBParticleSystem_BurstConfiguration.encode(message.bursts, writer.uint32(234).fork()).ldelim();
        }
        return writer;
    }
    PBParticleSystem.encode = encode;
    function decode(input, length) {
        const reader = input instanceof _m0.Reader ? input : _m0.Reader.create(input);
        let end = length === undefined ? reader.len : reader.pos + length;
        const message = createBasePBParticleSystem();
        while (reader.pos < end) {
            const tag = reader.uint32();
            switch (tag >>> 3) {
                case 1:
                    if (tag !== 8) {
                        break;
                    }
                    message.active = reader.bool();
                    continue;
                case 2:
                    if (tag !== 21) {
                        break;
                    }
                    message.rate = reader.float();
                    continue;
                case 3:
                    if (tag !== 24) {
                        break;
                    }
                    message.maxParticles = reader.uint32();
                    continue;
                case 4:
                    if (tag !== 37) {
                        break;
                    }
                    message.lifetime = reader.float();
                    continue;
                case 5:
                    if (tag !== 45) {
                        break;
                    }
                    message.gravity = reader.float();
                    continue;
                case 6:
                    if (tag !== 50) {
                        break;
                    }
                    message.additionalForce = Vector3.decode(reader, reader.uint32());
                    continue;
                case 7:
                    if (tag !== 58) {
                        break;
                    }
                    message.initialSize = FloatRange.decode(reader, reader.uint32());
                    continue;
                case 8:
                    if (tag !== 66) {
                        break;
                    }
                    message.sizeOverTime = FloatRange.decode(reader, reader.uint32());
                    continue;
                case 9:
                    if (tag !== 74) {
                        break;
                    }
                    message.initialRotation = Quaternion.decode(reader, reader.uint32());
                    continue;
                case 10:
                    if (tag !== 82) {
                        break;
                    }
                    message.rotationOverTime = Quaternion.decode(reader, reader.uint32());
                    continue;
                case 28:
                    if (tag !== 224) {
                        break;
                    }
                    message.faceTravelDirection = reader.bool();
                    continue;
                case 11:
                    if (tag !== 90) {
                        break;
                    }
                    message.initialColor = ColorRange.decode(reader, reader.uint32());
                    continue;
                case 12:
                    if (tag !== 98) {
                        break;
                    }
                    message.colorOverTime = ColorRange.decode(reader, reader.uint32());
                    continue;
                case 13:
                    if (tag !== 106) {
                        break;
                    }
                    message.initialVelocitySpeed = FloatRange.decode(reader, reader.uint32());
                    continue;
                case 14:
                    if (tag !== 114) {
                        break;
                    }
                    message.texture = Texture.decode(reader, reader.uint32());
                    continue;
                case 15:
                    if (tag !== 120) {
                        break;
                    }
                    message.blendMode = reader.int32();
                    continue;
                case 16:
                    if (tag !== 128) {
                        break;
                    }
                    message.billboard = reader.bool();
                    continue;
                case 17:
                    if (tag !== 138) {
                        break;
                    }
                    message.spriteSheet = PBParticleSystem_SpriteSheetAnimation.decode(reader, reader.uint32());
                    continue;
                case 18:
                    if (tag !== 146) {
                        break;
                    }
                    message.shape = { $case: "point", point: PBParticleSystem_Point.decode(reader, reader.uint32()) };
                    continue;
                case 19:
                    if (tag !== 154) {
                        break;
                    }
                    message.shape = { $case: "sphere", sphere: PBParticleSystem_Sphere.decode(reader, reader.uint32()) };
                    continue;
                case 20:
                    if (tag !== 162) {
                        break;
                    }
                    message.shape = { $case: "cone", cone: PBParticleSystem_Cone.decode(reader, reader.uint32()) };
                    continue;
                case 21:
                    if (tag !== 170) {
                        break;
                    }
                    message.shape = { $case: "box", box: PBParticleSystem_Box.decode(reader, reader.uint32()) };
                    continue;
                case 24:
                    if (tag !== 192) {
                        break;
                    }
                    message.loop = reader.bool();
                    continue;
                case 25:
                    if (tag !== 200) {
                        break;
                    }
                    message.prewarm = reader.bool();
                    continue;
                case 27:
                    if (tag !== 216) {
                        break;
                    }
                    message.simulationSpace = reader.int32();
                    continue;
                case 26:
                    if (tag !== 210) {
                        break;
                    }
                    message.limitVelocity = PBParticleSystem_LimitVelocity.decode(reader, reader.uint32());
                    continue;
                case 22:
                    if (tag !== 176) {
                        break;
                    }
                    message.playbackState = reader.int32();
                    continue;
                case 29:
                    if (tag !== 234) {
                        break;
                    }
                    message.bursts = PBParticleSystem_BurstConfiguration.decode(reader, reader.uint32());
                    continue;
            }
            if ((tag & 7) === 4 || tag === 0) {
                break;
            }
            reader.skipType(tag & 7);
        }
        return message;
    }
    PBParticleSystem.decode = decode;
})(PBParticleSystem || (PBParticleSystem = {}));
function createBasePBParticleSystem_SpriteSheetAnimation() {
    return { tilesX: 0, tilesY: 0, framesPerSecond: undefined };
}
/**
 * @public
 */
export var PBParticleSystem_SpriteSheetAnimation;
(function (PBParticleSystem_SpriteSheetAnimation) {
    function encode(message, writer = _m0.Writer.create()) {
        if (message.tilesX !== 0) {
            writer.uint32(8).uint32(message.tilesX);
        }
        if (message.tilesY !== 0) {
            writer.uint32(16).uint32(message.tilesY);
        }
        if (message.framesPerSecond !== undefined) {
            writer.uint32(29).float(message.framesPerSecond);
        }
        return writer;
    }
    PBParticleSystem_SpriteSheetAnimation.encode = encode;
    function decode(input, length) {
        const reader = input instanceof _m0.Reader ? input : _m0.Reader.create(input);
        let end = length === undefined ? reader.len : reader.pos + length;
        const message = createBasePBParticleSystem_SpriteSheetAnimation();
        while (reader.pos < end) {
            const tag = reader.uint32();
            switch (tag >>> 3) {
                case 1:
                    if (tag !== 8) {
                        break;
                    }
                    message.tilesX = reader.uint32();
                    continue;
                case 2:
                    if (tag !== 16) {
                        break;
                    }
                    message.tilesY = reader.uint32();
                    continue;
                case 3:
                    if (tag !== 29) {
                        break;
                    }
                    message.framesPerSecond = reader.float();
                    continue;
            }
            if ((tag & 7) === 4 || tag === 0) {
                break;
            }
            reader.skipType(tag & 7);
        }
        return message;
    }
    PBParticleSystem_SpriteSheetAnimation.decode = decode;
})(PBParticleSystem_SpriteSheetAnimation || (PBParticleSystem_SpriteSheetAnimation = {}));
function createBasePBParticleSystem_LimitVelocity() {
    return { speed: 0, dampen: undefined };
}
/**
 * @public
 */
export var PBParticleSystem_LimitVelocity;
(function (PBParticleSystem_LimitVelocity) {
    function encode(message, writer = _m0.Writer.create()) {
        if (message.speed !== 0) {
            writer.uint32(13).float(message.speed);
        }
        if (message.dampen !== undefined) {
            writer.uint32(21).float(message.dampen);
        }
        return writer;
    }
    PBParticleSystem_LimitVelocity.encode = encode;
    function decode(input, length) {
        const reader = input instanceof _m0.Reader ? input : _m0.Reader.create(input);
        let end = length === undefined ? reader.len : reader.pos + length;
        const message = createBasePBParticleSystem_LimitVelocity();
        while (reader.pos < end) {
            const tag = reader.uint32();
            switch (tag >>> 3) {
                case 1:
                    if (tag !== 13) {
                        break;
                    }
                    message.speed = reader.float();
                    continue;
                case 2:
                    if (tag !== 21) {
                        break;
                    }
                    message.dampen = reader.float();
                    continue;
            }
            if ((tag & 7) === 4 || tag === 0) {
                break;
            }
            reader.skipType(tag & 7);
        }
        return message;
    }
    PBParticleSystem_LimitVelocity.decode = decode;
})(PBParticleSystem_LimitVelocity || (PBParticleSystem_LimitVelocity = {}));
function createBasePBParticleSystem_Point() {
    return {};
}
/**
 * @public
 */
export var PBParticleSystem_Point;
(function (PBParticleSystem_Point) {
    function encode(_, writer = _m0.Writer.create()) {
        return writer;
    }
    PBParticleSystem_Point.encode = encode;
    function decode(input, length) {
        const reader = input instanceof _m0.Reader ? input : _m0.Reader.create(input);
        let end = length === undefined ? reader.len : reader.pos + length;
        const message = createBasePBParticleSystem_Point();
        while (reader.pos < end) {
            const tag = reader.uint32();
            switch (tag >>> 3) {
            }
            if ((tag & 7) === 4 || tag === 0) {
                break;
            }
            reader.skipType(tag & 7);
        }
        return message;
    }
    PBParticleSystem_Point.decode = decode;
})(PBParticleSystem_Point || (PBParticleSystem_Point = {}));
function createBasePBParticleSystem_Sphere() {
    return { radius: undefined };
}
/**
 * @public
 */
export var PBParticleSystem_Sphere;
(function (PBParticleSystem_Sphere) {
    function encode(message, writer = _m0.Writer.create()) {
        if (message.radius !== undefined) {
            writer.uint32(13).float(message.radius);
        }
        return writer;
    }
    PBParticleSystem_Sphere.encode = encode;
    function decode(input, length) {
        const reader = input instanceof _m0.Reader ? input : _m0.Reader.create(input);
        let end = length === undefined ? reader.len : reader.pos + length;
        const message = createBasePBParticleSystem_Sphere();
        while (reader.pos < end) {
            const tag = reader.uint32();
            switch (tag >>> 3) {
                case 1:
                    if (tag !== 13) {
                        break;
                    }
                    message.radius = reader.float();
                    continue;
            }
            if ((tag & 7) === 4 || tag === 0) {
                break;
            }
            reader.skipType(tag & 7);
        }
        return message;
    }
    PBParticleSystem_Sphere.decode = decode;
})(PBParticleSystem_Sphere || (PBParticleSystem_Sphere = {}));
function createBasePBParticleSystem_Cone() {
    return { angle: undefined, radius: undefined };
}
/**
 * @public
 */
export var PBParticleSystem_Cone;
(function (PBParticleSystem_Cone) {
    function encode(message, writer = _m0.Writer.create()) {
        if (message.angle !== undefined) {
            writer.uint32(13).float(message.angle);
        }
        if (message.radius !== undefined) {
            writer.uint32(21).float(message.radius);
        }
        return writer;
    }
    PBParticleSystem_Cone.encode = encode;
    function decode(input, length) {
        const reader = input instanceof _m0.Reader ? input : _m0.Reader.create(input);
        let end = length === undefined ? reader.len : reader.pos + length;
        const message = createBasePBParticleSystem_Cone();
        while (reader.pos < end) {
            const tag = reader.uint32();
            switch (tag >>> 3) {
                case 1:
                    if (tag !== 13) {
                        break;
                    }
                    message.angle = reader.float();
                    continue;
                case 2:
                    if (tag !== 21) {
                        break;
                    }
                    message.radius = reader.float();
                    continue;
            }
            if ((tag & 7) === 4 || tag === 0) {
                break;
            }
            reader.skipType(tag & 7);
        }
        return message;
    }
    PBParticleSystem_Cone.decode = decode;
})(PBParticleSystem_Cone || (PBParticleSystem_Cone = {}));
function createBasePBParticleSystem_Box() {
    return { size: undefined };
}
/**
 * @public
 */
export var PBParticleSystem_Box;
(function (PBParticleSystem_Box) {
    function encode(message, writer = _m0.Writer.create()) {
        if (message.size !== undefined) {
            Vector3.encode(message.size, writer.uint32(10).fork()).ldelim();
        }
        return writer;
    }
    PBParticleSystem_Box.encode = encode;
    function decode(input, length) {
        const reader = input instanceof _m0.Reader ? input : _m0.Reader.create(input);
        let end = length === undefined ? reader.len : reader.pos + length;
        const message = createBasePBParticleSystem_Box();
        while (reader.pos < end) {
            const tag = reader.uint32();
            switch (tag >>> 3) {
                case 1:
                    if (tag !== 10) {
                        break;
                    }
                    message.size = Vector3.decode(reader, reader.uint32());
                    continue;
            }
            if ((tag & 7) === 4 || tag === 0) {
                break;
            }
            reader.skipType(tag & 7);
        }
        return message;
    }
    PBParticleSystem_Box.decode = decode;
})(PBParticleSystem_Box || (PBParticleSystem_Box = {}));
function createBasePBParticleSystem_BurstConfiguration() {
    return { values: [] };
}
/**
 * @public
 */
export var PBParticleSystem_BurstConfiguration;
(function (PBParticleSystem_BurstConfiguration) {
    function encode(message, writer = _m0.Writer.create()) {
        for (const v of message.values) {
            PBParticleSystem_Burst.encode(v, writer.uint32(10).fork()).ldelim();
        }
        return writer;
    }
    PBParticleSystem_BurstConfiguration.encode = encode;
    function decode(input, length) {
        const reader = input instanceof _m0.Reader ? input : _m0.Reader.create(input);
        let end = length === undefined ? reader.len : reader.pos + length;
        const message = createBasePBParticleSystem_BurstConfiguration();
        while (reader.pos < end) {
            const tag = reader.uint32();
            switch (tag >>> 3) {
                case 1:
                    if (tag !== 10) {
                        break;
                    }
                    message.values.push(PBParticleSystem_Burst.decode(reader, reader.uint32()));
                    continue;
            }
            if ((tag & 7) === 4 || tag === 0) {
                break;
            }
            reader.skipType(tag & 7);
        }
        return message;
    }
    PBParticleSystem_BurstConfiguration.decode = decode;
})(PBParticleSystem_BurstConfiguration || (PBParticleSystem_BurstConfiguration = {}));
function createBasePBParticleSystem_Burst() {
    return { time: 0, count: 0, cycles: undefined, interval: undefined, probability: undefined };
}
/**
 * @public
 */
export var PBParticleSystem_Burst;
(function (PBParticleSystem_Burst) {
    function encode(message, writer = _m0.Writer.create()) {
        if (message.time !== 0) {
            writer.uint32(13).float(message.time);
        }
        if (message.count !== 0) {
            writer.uint32(16).uint32(message.count);
        }
        if (message.cycles !== undefined) {
            writer.uint32(24).int32(message.cycles);
        }
        if (message.interval !== undefined) {
            writer.uint32(37).float(message.interval);
        }
        if (message.probability !== undefined) {
            writer.uint32(45).float(message.probability);
        }
        return writer;
    }
    PBParticleSystem_Burst.encode = encode;
    function decode(input, length) {
        const reader = input instanceof _m0.Reader ? input : _m0.Reader.create(input);
        let end = length === undefined ? reader.len : reader.pos + length;
        const message = createBasePBParticleSystem_Burst();
        while (reader.pos < end) {
            const tag = reader.uint32();
            switch (tag >>> 3) {
                case 1:
                    if (tag !== 13) {
                        break;
                    }
                    message.time = reader.float();
                    continue;
                case 2:
                    if (tag !== 16) {
                        break;
                    }
                    message.count = reader.uint32();
                    continue;
                case 3:
                    if (tag !== 24) {
                        break;
                    }
                    message.cycles = reader.int32();
                    continue;
                case 4:
                    if (tag !== 37) {
                        break;
                    }
                    message.interval = reader.float();
                    continue;
                case 5:
                    if (tag !== 45) {
                        break;
                    }
                    message.probability = reader.float();
                    continue;
            }
            if ((tag & 7) === 4 || tag === 0) {
                break;
            }
            reader.skipType(tag & 7);
        }
        return message;
    }
    PBParticleSystem_Burst.decode = decode;
})(PBParticleSystem_Burst || (PBParticleSystem_Burst = {}));
