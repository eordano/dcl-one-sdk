import _m0 from "protobufjs/minimal";
import { ColorRange } from "../../common/colors.gen";
import { FloatRange } from "../../common/floats.gen";
import { Texture } from "../../common/texture.gen";
import { Quaternion, Vector3 } from "../../common/vectors.gen";
/**
 * @public
 */
export interface PBParticleSystem {
    /** --- Emission --- */
    active?: boolean | undefined;
    /** default = 10. Particles emitted per second. */
    rate?: number | undefined;
    /** default = 1000. Maximum number of live particles. */
    maxParticles?: number | undefined;
    /** default = 5. Particle lifetime in seconds. */
    lifetime?: number | undefined;
    /** --- Motion --- */
    gravity?: number | undefined;
    /** Constant force vector applied to each particle (world space). */
    additionalForce?: Vector3 | undefined;
    /** --- Size --- */
    initialSize?: FloatRange | undefined;
    /** default = {1, 1}. Particle size lerped from start to end over its lifetime. */
    sizeOverTime?: FloatRange | undefined;
    /** --- Rotation --- */
    initialRotation?: Quaternion | undefined;
    /** default = identity (0,0,0,1). Per-axis angular velocity as quaternion; converted to Euler XYZ. */
    rotationOverTime?: Quaternion | undefined;
    /** default = false. Particles orient along their velocity direction. */
    faceTravelDirection?: boolean | undefined;
    /** --- Color --- */
    initialColor?: ColorRange | undefined;
    /** default = {white, white}. Particle color lerped from start to end over its lifetime. */
    colorOverTime?: ColorRange | undefined;
    /** --- Velocity --- */
    initialVelocitySpeed?: FloatRange | undefined;
    /** --- Rendering --- */
    texture?: Texture | undefined;
    /** default = PSB_ALPHA */
    blendMode?: PBParticleSystem_BlendMode | undefined;
    /** default = true */
    billboard?: boolean | undefined;
    /** --- Sprite Sheet Animation --- */
    spriteSheet?: PBParticleSystem_SpriteSheetAnimation | undefined;
    shape?: {
        $case: "point";
        point: PBParticleSystem_Point;
    } | {
        $case: "sphere";
        sphere: PBParticleSystem_Sphere;
    } | {
        $case: "cone";
        cone: PBParticleSystem_Cone;
    } | {
        $case: "box";
        box: PBParticleSystem_Box;
    } | undefined;
    /** --- Simulation --- */
    loop?: boolean | undefined;
    /** default = false. Start as if already simulated for one full loop cycle. Requires loop = true. */
    prewarm?: boolean | undefined;
    /** default = PSS_LOCAL. Controls whether particles simulate in local or world space. */
    simulationSpace?: PBParticleSystem_SimulationSpace | undefined;
    /** --- Limit Velocity Over Lifetime --- */
    limitVelocity?: PBParticleSystem_LimitVelocity | undefined;
    /** --- Playback --- */
    playbackState?: PBParticleSystem_PlaybackState | undefined;
    /** --- Emission Bursts --- */
    bursts?: PBParticleSystem_BurstConfiguration | undefined;
}
/**
 * @public
 */
export declare const enum PBParticleSystem_BlendMode {
    /** PSB_ALPHA - Standard alpha transparency. */
    PSB_ALPHA = 0,
    /** PSB_ADD - Additive blending (brightens underlying pixels). */
    PSB_ADD = 1,
    /** PSB_MULTIPLY - Multiply blending (darkens underlying pixels). */
    PSB_MULTIPLY = 2
}
/**
 * @public
 */
export declare const enum PBParticleSystem_PlaybackState {
    /** PS_PLAYING - Particle system is emitting and simulating. */
    PS_PLAYING = 0,
    /** PS_PAUSED - Simulation is frozen; no new particles are emitted. */
    PS_PAUSED = 1,
    /** PS_STOPPED - Simulation stopped and existing particles cleared. */
    PS_STOPPED = 2
}
/**
 * @public
 */
export declare const enum PBParticleSystem_SimulationSpace {
    /** PSS_LOCAL - Particles move with the entity transform. */
    PSS_LOCAL = 0,
    /** PSS_WORLD - Particles stay in world position after emission. */
    PSS_WORLD = 1
}
/** Sprite sheet (texture atlas) animation settings. */
/**
 * @public
 */
export interface PBParticleSystem_SpriteSheetAnimation {
    /** Number of columns in the sprite sheet. */
    tilesX: number;
    /** Number of rows in the sprite sheet. */
    tilesY: number;
    /** default = 30. Playback speed in frames per second. */
    framesPerSecond?: number | undefined;
}
/** Clamps particle speed over lifetime. */
/**
 * @public
 */
export interface PBParticleSystem_LimitVelocity {
    /** Maximum particle speed (m/s). */
    speed: number;
    /** default = 1. Fraction (0–1) of excess velocity removed per frame. 1 = hard clamp. */
    dampen?: number | undefined;
}
/** Emitter spawns particles from a single point. */
/**
 * @public
 */
export interface PBParticleSystem_Point {
}
/** Emitter spawns particles from the surface or volume of a sphere. */
/**
 * @public
 */
export interface PBParticleSystem_Sphere {
    /** default = 1 */
    radius?: number | undefined;
}
/** Emitter spawns particles from the base of a cone and projects them outward. */
/**
 * @public
 */
export interface PBParticleSystem_Cone {
    /** default = 25. Half-angle of the cone in degrees. */
    angle?: number | undefined;
    /** default = 1. Base radius in meters. */
    radius?: number | undefined;
}
/** Emitter spawns particles from the surface or volume of a box. */
/**
 * @public
 */
export interface PBParticleSystem_Box {
    /** default = {1, 1, 1} */
    size?: Vector3 | undefined;
}
/** Emission burst configuration. */
/**
 * @public
 */
export interface PBParticleSystem_BurstConfiguration {
    values: PBParticleSystem_Burst[];
}
/**
 * @public
 */
export interface PBParticleSystem_Burst {
    /** Seconds from start of cycle. */
    time: number;
    /** Particles to emit. */
    count: number;
    /** default = 1. Repeat count (0 = infinite). */
    cycles?: number | undefined;
    /** default = 0.01. Seconds between cycles. */
    interval?: number | undefined;
    /** default = 1.0. Emission chance [0,1]. */
    probability?: number | undefined;
}
/**
 * @public
 */
export declare namespace PBParticleSystem {
    function encode(message: PBParticleSystem, writer?: _m0.Writer): _m0.Writer;
    function decode(input: _m0.Reader | Uint8Array, length?: number): PBParticleSystem;
}
/**
 * @public
 */
export declare namespace PBParticleSystem_SpriteSheetAnimation {
    function encode(message: PBParticleSystem_SpriteSheetAnimation, writer?: _m0.Writer): _m0.Writer;
    function decode(input: _m0.Reader | Uint8Array, length?: number): PBParticleSystem_SpriteSheetAnimation;
}
/**
 * @public
 */
export declare namespace PBParticleSystem_LimitVelocity {
    function encode(message: PBParticleSystem_LimitVelocity, writer?: _m0.Writer): _m0.Writer;
    function decode(input: _m0.Reader | Uint8Array, length?: number): PBParticleSystem_LimitVelocity;
}
/**
 * @public
 */
export declare namespace PBParticleSystem_Point {
    function encode(_: PBParticleSystem_Point, writer?: _m0.Writer): _m0.Writer;
    function decode(input: _m0.Reader | Uint8Array, length?: number): PBParticleSystem_Point;
}
/**
 * @public
 */
export declare namespace PBParticleSystem_Sphere {
    function encode(message: PBParticleSystem_Sphere, writer?: _m0.Writer): _m0.Writer;
    function decode(input: _m0.Reader | Uint8Array, length?: number): PBParticleSystem_Sphere;
}
/**
 * @public
 */
export declare namespace PBParticleSystem_Cone {
    function encode(message: PBParticleSystem_Cone, writer?: _m0.Writer): _m0.Writer;
    function decode(input: _m0.Reader | Uint8Array, length?: number): PBParticleSystem_Cone;
}
/**
 * @public
 */
export declare namespace PBParticleSystem_Box {
    function encode(message: PBParticleSystem_Box, writer?: _m0.Writer): _m0.Writer;
    function decode(input: _m0.Reader | Uint8Array, length?: number): PBParticleSystem_Box;
}
/**
 * @public
 */
export declare namespace PBParticleSystem_BurstConfiguration {
    function encode(message: PBParticleSystem_BurstConfiguration, writer?: _m0.Writer): _m0.Writer;
    function decode(input: _m0.Reader | Uint8Array, length?: number): PBParticleSystem_BurstConfiguration;
}
/**
 * @public
 */
export declare namespace PBParticleSystem_Burst {
    function encode(message: PBParticleSystem_Burst, writer?: _m0.Writer): _m0.Writer;
    function decode(input: _m0.Reader | Uint8Array, length?: number): PBParticleSystem_Burst;
}
