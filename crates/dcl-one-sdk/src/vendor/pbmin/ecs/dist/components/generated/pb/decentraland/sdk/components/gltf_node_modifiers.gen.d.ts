import _m0 from "protobufjs/minimal";
import { PBMaterial } from "./material.gen";
/**
 * GltfNodeModifiers component is to be used attached to entities that have the GltfContainer component.
 *
 * This allows to override either the Material or the Casting Shadows behaviour of the target GLTF Node.
 *
 * * If the 'path' of the first modifier in the collection is an empty string: the configuration will
 * affect all of the GLTF Nodes (as a global modifier).
 * * Otherwise, for the modifiers whose 'path' is found in the GLTF hierarchy, the modifier will affect only
 * the target Nodes.
 */
/**
 * @public
 */
export interface PBGltfNodeModifiers {
    modifiers: PBGltfNodeModifiers_GltfNodeModifier[];
}
/**
 * @public
 */
export interface PBGltfNodeModifiers_GltfNodeModifier {
    /** The GLTF hierarchy path of the target Node to be affected */
    path: string;
    /** The casting shadows enabled override */
    castShadows?: boolean | undefined;
    /** The Material that will be overridden on the target Node */
    material?: PBMaterial | undefined;
}
/**
 * @public
 */
export declare namespace PBGltfNodeModifiers {
    function encode(message: PBGltfNodeModifiers, writer?: _m0.Writer): _m0.Writer;
    function decode(input: _m0.Reader | Uint8Array, length?: number): PBGltfNodeModifiers;
}
/**
 * @public
 */
export declare namespace PBGltfNodeModifiers_GltfNodeModifier {
    function encode(message: PBGltfNodeModifiers_GltfNodeModifier, writer?: _m0.Writer): _m0.Writer;
    function decode(input: _m0.Reader | Uint8Array, length?: number): PBGltfNodeModifiers_GltfNodeModifier;
}
