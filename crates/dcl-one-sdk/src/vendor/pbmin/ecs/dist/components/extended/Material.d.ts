import { LastWriteWinElementSetComponentDefinition, Entity, IEngine } from '../../engine';
import { MaterialTransparencyMode, PBMaterial, PBMaterial_PbrMaterial, PBMaterial_UnlitMaterial } from '../generated/index.gen';
import { AvatarTexture, Texture, TextureFilterMode, TextureUnion, TextureWrapMode, VideoTexture } from '../generated/types.gen';
import { Color3, Color4 } from '../generated/pb/decentraland/common/colors.gen';
/**
 * @public
 */
export interface TextureHelper {
    /**
     * @returns a common texture with a source file
     */
    Common: (texture: Texture) => TextureUnion;
    /**
     * @returns the avatar texture of userId specified
     */
    Avatar: (avatarTexture: AvatarTexture) => TextureUnion;
    /**
     * @returns the video texture of videoPlayerEntity specified
     */
    Video: (videoTexture: VideoTexture) => TextureUnion;
}
/**
 * Flattened texture interface for simplified property access
 * @public
 */
export interface FlatTexture {
    /** Path to the texture file */
    src: string | undefined;
    /** Texture wrapping behavior */
    wrapMode: TextureWrapMode | undefined;
    /** Texture filtering mode */
    filterMode: TextureFilterMode | undefined;
}
/**
 * Flattened material interface for simplified property access
 * @public
 */
export interface FlatMaterial {
    /**
     * Access to the main texture properties (works for both PBR and Unlit materials)
     */
    readonly texture: FlatTexture;
    /**
     * Access to the alpha texture properties (works for both PBR and Unlit materials)
     */
    readonly alphaTexture: FlatTexture;
    /**
     * Access to the emissive texture properties (PBR only - returns undefined for Unlit materials)
     */
    readonly emissiveTexture: FlatTexture | undefined;
    /**
     * Access to the bump/normal texture properties (PBR only - returns undefined for Unlit materials)
     */
    readonly bumpTexture: FlatTexture | undefined;
    /**
     * Alpha test threshold (0-1). Default: 0.5
     */
    alphaTest: number | undefined;
    /**
     * Whether the material casts shadows. Default: true
     */
    castShadows: boolean | undefined;
    /**
     * Albedo/base color (PBR only). Default: white
     */
    albedoColor: Color4 | undefined;
    /**
     * Emissive color (PBR only). Default: black
     */
    emissiveColor: Color3 | undefined;
    /**
     * Reflectivity color (PBR only). Default: white
     */
    reflectivityColor: Color3 | undefined;
    /**
     * Transparency mode (PBR only). Default: MTM_AUTO
     */
    transparencyMode: MaterialTransparencyMode | undefined;
    /**
     * Metallic value 0-1 (PBR only). Default: 0.5
     */
    metallic: number | undefined;
    /**
     * Roughness value 0-1 (PBR only). Default: 0.5
     */
    roughness: number | undefined;
    /**
     * Specular intensity (PBR only). Default: 1
     */
    specularIntensity: number | undefined;
    /**
     * Emissive intensity (PBR only). Default: 2
     */
    emissiveIntensity: number | undefined;
    /**
     * Direct light intensity (PBR only). Default: 1
     */
    directIntensity: number | undefined;
    /**
     * Diffuse color (Unlit only). Default: white
     */
    diffuseColor: Color4 | undefined;
}
/**
 * Readonly flattened texture interface for read-only property access
 * @public
 */
export interface ReadonlyFlatTexture {
    /** Path to the texture file */
    readonly src: string | undefined;
    /** Texture wrapping behavior */
    readonly wrapMode: TextureWrapMode | undefined;
    /** Texture filtering mode */
    readonly filterMode: TextureFilterMode | undefined;
}
/**
 * Readonly flattened material interface for read-only property access
 * @public
 */
export interface ReadonlyFlatMaterial {
    /**
     * Access to the main texture properties (works for both PBR and Unlit materials)
     */
    readonly texture: ReadonlyFlatTexture;
    /**
     * Access to the alpha texture properties (works for both PBR and Unlit materials)
     */
    readonly alphaTexture: ReadonlyFlatTexture;
    /**
     * Access to the emissive texture properties (PBR only - returns undefined for Unlit materials)
     */
    readonly emissiveTexture: ReadonlyFlatTexture | undefined;
    /**
     * Access to the bump/normal texture properties (PBR only - returns undefined for Unlit materials)
     */
    readonly bumpTexture: ReadonlyFlatTexture | undefined;
    /**
     * Alpha test threshold (0-1). Default: 0.5
     */
    readonly alphaTest: number | undefined;
    /**
     * Whether the material casts shadows. Default: true
     */
    readonly castShadows: boolean | undefined;
    /**
     * Albedo/base color (PBR only). Default: white
     */
    readonly albedoColor: Color4 | undefined;
    /**
     * Emissive color (PBR only). Default: black
     */
    readonly emissiveColor: Color3 | undefined;
    /**
     * Reflectivity color (PBR only). Default: white
     */
    readonly reflectivityColor: Color3 | undefined;
    /**
     * Transparency mode (PBR only). Default: MTM_AUTO
     */
    readonly transparencyMode: MaterialTransparencyMode | undefined;
    /**
     * Metallic value 0-1 (PBR only). Default: 0.5
     */
    readonly metallic: number | undefined;
    /**
     * Roughness value 0-1 (PBR only). Default: 0.5
     */
    readonly roughness: number | undefined;
    /**
     * Specular intensity (PBR only). Default: 1
     */
    readonly specularIntensity: number | undefined;
    /**
     * Emissive intensity (PBR only). Default: 2
     */
    readonly emissiveIntensity: number | undefined;
    /**
     * Direct light intensity (PBR only). Default: 1
     */
    readonly directIntensity: number | undefined;
    /**
     * Diffuse color (Unlit only). Default: white
     */
    readonly diffuseColor: Color4 | undefined;
}
/**
 * @public
 */
export interface MaterialComponentDefinitionExtended extends LastWriteWinElementSetComponentDefinition<PBMaterial> {
    /**
     * Texture helpers with constructor
     */
    Texture: TextureHelper;
    /**
     * Create or replace the component Material in the entity specified
     * @param entity - the entity to link the component
     * @param material - the Unlit data for this material
     */
    setBasicMaterial: (entity: Entity, material: PBMaterial_UnlitMaterial) => void;
    /**
     * Create or replace the component Material in the entity specified
     * @param entity - the entity to link the component
     * @param material - the PBR data for this material
     */
    setPbrMaterial: (entity: Entity, material: PBMaterial_PbrMaterial) => void;
    /**
     * Get a readonly flattened accessor for the material's properties.
     * Simplifies reading material properties without navigating nested unions.
     * Works for both PBR and Unlit materials.
     * Throws if the entity does not have a Material component.
     * @param entity - Entity with the Material component
     * @returns A readonly accessor object with direct access to material properties
     *
     */
    getFlat: (entity: Entity) => ReadonlyFlatMaterial;
    /**
     * Get a readonly flattened accessor for the material's properties, or null if the entity
     * does not have a Material component.
     * @param entity - Entity to check for Material component
     * @returns A readonly accessor object, or null if no Material component exists
     *
     */
    getFlatOrNull: (entity: Entity) => ReadonlyFlatMaterial | null;
    /**
     * Get a mutable flattened accessor for the material's properties.
     * Simplifies reading/writing material properties without navigating nested unions.
     * Works for both PBR and Unlit materials.
     * Throws if the entity does not have a Material component.
     * @param entity - Entity with the Material component
     * @returns A mutable accessor object with direct access to material properties
     *
     */
    getFlatMutable: (entity: Entity) => FlatMaterial;
    /**
     * Get a mutable flattened accessor for the material's properties, or null if the entity
     * does not have a Material component.
     * @param entity - Entity to check for Material component
     * @returns A mutable accessor object, or null if no Material component exists
     *
     */
    getFlatMutableOrNull: (entity: Entity) => FlatMaterial | null;
}
export declare function defineMaterialComponent(engine: Pick<IEngine, 'defineComponentFromSchema'>): MaterialComponentDefinitionExtended;
