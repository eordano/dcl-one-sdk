import { Material } from '../generated/index.gen';
const TextureHelper = {
    Common(texture) {
        return {
            tex: {
                $case: 'texture',
                texture
            }
        };
    },
    Avatar(avatarTexture) {
        return {
            tex: {
                $case: 'avatarTexture',
                avatarTexture
            }
        };
    },
    Video(videoTexture) {
        return {
            tex: {
                $case: 'videoTexture',
                videoTexture
            }
        };
    }
};
/**
 * Helper to get the inner material object (pbr or unlit) from a PBMaterial
 */
function getInnerMaterial(mat) {
    if (mat.material?.$case === 'pbr') {
        return mat.material.pbr;
    }
    else if (mat.material?.$case === 'unlit') {
        return mat.material.unlit;
    }
    return undefined;
}
/**
 * Helper to get the Texture object from a TextureUnion (only for regular textures)
 */
function getTextureFromUnion(textureUnion) {
    if (textureUnion?.tex?.$case === 'texture') {
        return textureUnion.tex.texture;
    }
    return undefined;
}
/**
 * Check if a texture field is PBR-only
 */
function isPbrOnlyTexture(field) {
    return field === 'emissiveTexture' || field === 'bumpTexture';
}
/**
 * Get the texture union from the inner material based on field name
 */
function getTextureField(innerMat, field, isPbr) {
    if (!innerMat)
        return undefined;
    // For PBR-only textures on Unlit materials, return undefined
    if (isPbrOnlyTexture(field) && !isPbr) {
        return undefined;
    }
    switch (field) {
        case 'texture':
            return innerMat.texture;
        case 'alphaTexture':
            return innerMat.alphaTexture;
        case 'emissiveTexture':
            return innerMat.emissiveTexture;
        case 'bumpTexture':
            return innerMat.bumpTexture;
        default:
            return undefined;
    }
}
/**
 * Helper to validate that a texture is not an Avatar or Video texture
 * Throws an error if it is, since flat accessors only work with regular textures
 */
function validateNotSpecialTexture(textureUnion, field) {
    if (textureUnion?.tex?.$case === 'avatarTexture') {
        throw new Error(`Cannot set ${field} properties on Avatar texture. Use setPbrMaterial/setBasicMaterial with Material.Texture.Common() to set a regular texture.`);
    }
    if (textureUnion?.tex?.$case === 'videoTexture') {
        throw new Error(`Cannot set ${field} properties on Video texture. Use setPbrMaterial/setBasicMaterial with Material.Texture.Common() to set a regular texture.`);
    }
}
/**
 * Helper to ensure the material has a texture structure for the given field and return it for modification
 */
function ensureTextureStructure(mat, field) {
    if (!mat.material) {
        throw new Error('Material structure is invalid. Use setPbrMaterial or setBasicMaterial first.');
    }
    // Check if trying to set a PBR-only texture on Unlit material
    if (isPbrOnlyTexture(field) && mat.material.$case !== 'pbr') {
        throw new Error(`Cannot set ${field} on Unlit material. Use PBR material instead.`);
    }
    const innerMat = mat.material.$case === 'pbr' ? mat.material.pbr : mat.material.unlit;
    const isPbr = mat.material.$case === 'pbr';
    // Get or create the texture union for the specified field
    let textureUnion;
    switch (field) {
        case 'texture':
            validateNotSpecialTexture(innerMat.texture, field);
            if (!innerMat.texture || innerMat.texture.tex?.$case !== 'texture') {
                innerMat.texture = { tex: { $case: 'texture', texture: { src: '' } } };
            }
            textureUnion = innerMat.texture;
            break;
        case 'alphaTexture':
            validateNotSpecialTexture(innerMat.alphaTexture, field);
            if (!innerMat.alphaTexture || innerMat.alphaTexture.tex?.$case !== 'texture') {
                innerMat.alphaTexture = { tex: { $case: 'texture', texture: { src: '' } } };
            }
            textureUnion = innerMat.alphaTexture;
            break;
        case 'emissiveTexture':
            if (isPbr) {
                const pbrMat = innerMat;
                validateNotSpecialTexture(pbrMat.emissiveTexture, field);
                if (!pbrMat.emissiveTexture || pbrMat.emissiveTexture.tex?.$case !== 'texture') {
                    pbrMat.emissiveTexture = { tex: { $case: 'texture', texture: { src: '' } } };
                }
                textureUnion = pbrMat.emissiveTexture;
            }
            break;
        case 'bumpTexture':
            if (isPbr) {
                const pbrMat = innerMat;
                validateNotSpecialTexture(pbrMat.bumpTexture, field);
                if (!pbrMat.bumpTexture || pbrMat.bumpTexture.tex?.$case !== 'texture') {
                    pbrMat.bumpTexture = { tex: { $case: 'texture', texture: { src: '' } } };
                }
                textureUnion = pbrMat.bumpTexture;
            }
            break;
    }
    // At this point we know tex.$case is 'texture', but TypeScript doesn't narrow it so we use a type assertion
    const tex = textureUnion.tex;
    return tex.texture;
}
/**
 * Class-based accessor for FlatTexture properties.
 * Provides getters/setters that read/write to the nested material structure.
 */
class FlatTextureAccessor {
    constructor(getMaterial, field) {
        this.getMaterial = getMaterial;
        this.field = field;
    }
    get src() {
        const mat = this.getMaterial();
        const isPbr = mat.material?.$case === 'pbr';
        const innerMat = getInnerMaterial(mat);
        const textureUnion = getTextureField(innerMat, this.field, isPbr);
        const texture = getTextureFromUnion(textureUnion);
        return texture?.src;
    }
    set src(value) {
        if (value === undefined)
            return;
        const mat = this.getMaterial();
        const texture = ensureTextureStructure(mat, this.field);
        texture.src = value;
    }
    get wrapMode() {
        const mat = this.getMaterial();
        const isPbr = mat.material?.$case === 'pbr';
        const innerMat = getInnerMaterial(mat);
        const textureUnion = getTextureField(innerMat, this.field, isPbr);
        const texture = getTextureFromUnion(textureUnion);
        return texture?.wrapMode;
    }
    set wrapMode(value) {
        if (value === undefined)
            return;
        const mat = this.getMaterial();
        const texture = ensureTextureStructure(mat, this.field);
        texture.wrapMode = value;
    }
    get filterMode() {
        const mat = this.getMaterial();
        const isPbr = mat.material?.$case === 'pbr';
        const innerMat = getInnerMaterial(mat);
        const textureUnion = getTextureField(innerMat, this.field, isPbr);
        const texture = getTextureFromUnion(textureUnion);
        return texture?.filterMode;
    }
    set filterMode(value) {
        if (value === undefined)
            return;
        const mat = this.getMaterial();
        const texture = ensureTextureStructure(mat, this.field);
        texture.filterMode = value;
    }
}
/**
 * Class-based accessor for FlatMaterial properties.
 * Provides getters/setters for all material properties with proper type safety.
 */
class FlatMaterialAccessor {
    constructor(getMaterial) {
        this.getMaterial = getMaterial;
    }
    // ==================== Private Helpers ====================
    /**
     * Get PBR material if available, undefined otherwise
     */
    getPbrMaterial() {
        const mat = this.getMaterial();
        if (mat.material?.$case !== 'pbr')
            return undefined;
        return mat.material.pbr;
    }
    /**
     * Ensure material is PBR and return it, throw otherwise
     */
    ensurePbrMaterial(propertyName) {
        const mat = this.getMaterial();
        if (!mat.material) {
            throw new Error('Material structure is invalid. Use setPbrMaterial or setBasicMaterial first.');
        }
        if (mat.material.$case !== 'pbr') {
            throw new Error(`Cannot set ${propertyName} on Unlit material. Use PBR material instead.`);
        }
        return mat.material.pbr;
    }
    /**
     * Ensure inner material exists and return it, throw otherwise
     */
    ensureInnerMaterial() {
        const mat = this.getMaterial();
        if (!mat.material) {
            throw new Error('Material structure is invalid. Use setPbrMaterial or setBasicMaterial first.');
        }
        return mat.material.$case === 'pbr' ? mat.material.pbr : mat.material.unlit;
    }
    /**
     * Get Unlit material if available, undefined otherwise
     */
    getUnlitMaterial() {
        const mat = this.getMaterial();
        if (mat.material?.$case !== 'unlit')
            return undefined;
        return mat.material.unlit;
    }
    /**
     * Ensure material is Unlit and return it, throw otherwise
     */
    ensureUnlitMaterial(propertyName) {
        const mat = this.getMaterial();
        if (!mat.material) {
            throw new Error('Material structure is invalid. Use setPbrMaterial or setBasicMaterial first.');
        }
        if (mat.material.$case !== 'unlit') {
            throw new Error(`Cannot set ${propertyName} on PBR material. Use Unlit material instead.`);
        }
        return mat.material.unlit;
    }
    // ==================== Texture Accessors ====================
    get texture() {
        return new FlatTextureAccessor(this.getMaterial, 'texture');
    }
    get alphaTexture() {
        return new FlatTextureAccessor(this.getMaterial, 'alphaTexture');
    }
    get emissiveTexture() {
        const mat = this.getMaterial();
        if (mat.material?.$case !== 'pbr')
            return undefined;
        return new FlatTextureAccessor(this.getMaterial, 'emissiveTexture');
    }
    get bumpTexture() {
        const mat = this.getMaterial();
        if (mat.material?.$case !== 'pbr')
            return undefined;
        return new FlatTextureAccessor(this.getMaterial, 'bumpTexture');
    }
    // ==================== Shared Properties (PBR + Unlit) ====================
    get alphaTest() {
        return getInnerMaterial(this.getMaterial())?.alphaTest;
    }
    set alphaTest(value) {
        this.ensureInnerMaterial().alphaTest = value;
    }
    get castShadows() {
        return getInnerMaterial(this.getMaterial())?.castShadows;
    }
    set castShadows(value) {
        this.ensureInnerMaterial().castShadows = value;
    }
    // ==================== PBR-only Properties ====================
    get albedoColor() {
        return this.getPbrMaterial()?.albedoColor;
    }
    set albedoColor(value) {
        this.ensurePbrMaterial('albedoColor').albedoColor = value;
    }
    get emissiveColor() {
        return this.getPbrMaterial()?.emissiveColor;
    }
    set emissiveColor(value) {
        this.ensurePbrMaterial('emissiveColor').emissiveColor = value;
    }
    get reflectivityColor() {
        return this.getPbrMaterial()?.reflectivityColor;
    }
    set reflectivityColor(value) {
        this.ensurePbrMaterial('reflectivityColor').reflectivityColor = value;
    }
    get transparencyMode() {
        return this.getPbrMaterial()?.transparencyMode;
    }
    set transparencyMode(value) {
        this.ensurePbrMaterial('transparencyMode').transparencyMode = value;
    }
    get metallic() {
        return this.getPbrMaterial()?.metallic;
    }
    set metallic(value) {
        this.ensurePbrMaterial('metallic').metallic = value;
    }
    get roughness() {
        return this.getPbrMaterial()?.roughness;
    }
    set roughness(value) {
        this.ensurePbrMaterial('roughness').roughness = value;
    }
    get specularIntensity() {
        return this.getPbrMaterial()?.specularIntensity;
    }
    set specularIntensity(value) {
        this.ensurePbrMaterial('specularIntensity').specularIntensity = value;
    }
    get emissiveIntensity() {
        return this.getPbrMaterial()?.emissiveIntensity;
    }
    set emissiveIntensity(value) {
        this.ensurePbrMaterial('emissiveIntensity').emissiveIntensity = value;
    }
    get directIntensity() {
        return this.getPbrMaterial()?.directIntensity;
    }
    set directIntensity(value) {
        this.ensurePbrMaterial('directIntensity').directIntensity = value;
    }
    // ==================== Unlit-only Property ====================
    get diffuseColor() {
        return this.getUnlitMaterial()?.diffuseColor;
    }
    set diffuseColor(value) {
        this.ensureUnlitMaterial('diffuseColor').diffuseColor = value;
    }
}
/**
 * Readonly class-based accessor for FlatTexture properties.
 * Provides only getters for read-only access to the nested material structure.
 */
class ReadonlyFlatTextureAccessor {
    constructor(getMaterial, field) {
        this.getMaterial = getMaterial;
        this.field = field;
    }
    get src() {
        const mat = this.getMaterial();
        const isPbr = mat.material?.$case === 'pbr';
        const innerMat = getInnerMaterial(mat);
        const textureUnion = getTextureField(innerMat, this.field, isPbr);
        const texture = getTextureFromUnion(textureUnion);
        return texture?.src;
    }
    get wrapMode() {
        const mat = this.getMaterial();
        const isPbr = mat.material?.$case === 'pbr';
        const innerMat = getInnerMaterial(mat);
        const textureUnion = getTextureField(innerMat, this.field, isPbr);
        const texture = getTextureFromUnion(textureUnion);
        return texture?.wrapMode;
    }
    get filterMode() {
        const mat = this.getMaterial();
        const isPbr = mat.material?.$case === 'pbr';
        const innerMat = getInnerMaterial(mat);
        const textureUnion = getTextureField(innerMat, this.field, isPbr);
        const texture = getTextureFromUnion(textureUnion);
        return texture?.filterMode;
    }
}
/**
 * Readonly class-based accessor for FlatMaterial properties.
 * Provides only getters for read-only access to material properties.
 */
class ReadonlyFlatMaterialAccessor {
    constructor(getMaterial) {
        this.getMaterial = getMaterial;
    }
    // ==================== Private Helpers ====================
    /**
     * Get PBR material if available, undefined otherwise
     */
    getPbrMaterial() {
        const mat = this.getMaterial();
        if (mat.material?.$case !== 'pbr')
            return undefined;
        return mat.material.pbr;
    }
    /**
     * Get Unlit material if available, undefined otherwise
     */
    getUnlitMaterial() {
        const mat = this.getMaterial();
        if (mat.material?.$case !== 'unlit')
            return undefined;
        return mat.material.unlit;
    }
    // ==================== Texture Accessors ====================
    get texture() {
        return new ReadonlyFlatTextureAccessor(this.getMaterial, 'texture');
    }
    get alphaTexture() {
        return new ReadonlyFlatTextureAccessor(this.getMaterial, 'alphaTexture');
    }
    get emissiveTexture() {
        const mat = this.getMaterial();
        if (mat.material?.$case !== 'pbr')
            return undefined;
        return new ReadonlyFlatTextureAccessor(this.getMaterial, 'emissiveTexture');
    }
    get bumpTexture() {
        const mat = this.getMaterial();
        if (mat.material?.$case !== 'pbr')
            return undefined;
        return new ReadonlyFlatTextureAccessor(this.getMaterial, 'bumpTexture');
    }
    // ==================== Shared Properties (PBR + Unlit) ====================
    get alphaTest() {
        return getInnerMaterial(this.getMaterial())?.alphaTest;
    }
    get castShadows() {
        return getInnerMaterial(this.getMaterial())?.castShadows;
    }
    // ==================== PBR-only Properties ====================
    get albedoColor() {
        return this.getPbrMaterial()?.albedoColor;
    }
    get emissiveColor() {
        return this.getPbrMaterial()?.emissiveColor;
    }
    get reflectivityColor() {
        return this.getPbrMaterial()?.reflectivityColor;
    }
    get transparencyMode() {
        return this.getPbrMaterial()?.transparencyMode;
    }
    get metallic() {
        return this.getPbrMaterial()?.metallic;
    }
    get roughness() {
        return this.getPbrMaterial()?.roughness;
    }
    get specularIntensity() {
        return this.getPbrMaterial()?.specularIntensity;
    }
    get emissiveIntensity() {
        return this.getPbrMaterial()?.emissiveIntensity;
    }
    get directIntensity() {
        return this.getPbrMaterial()?.directIntensity;
    }
    // ==================== Unlit-only Property ====================
    get diffuseColor() {
        return this.getUnlitMaterial()?.diffuseColor;
    }
}
export function defineMaterialComponent(engine) {
    const theComponent = Material(engine);
    return {
        ...theComponent,
        Texture: TextureHelper,
        setBasicMaterial(entity, material) {
            theComponent.createOrReplace(entity, {
                material: {
                    $case: 'unlit',
                    unlit: material
                }
            });
        },
        setPbrMaterial(entity, material) {
            theComponent.createOrReplace(entity, {
                material: {
                    $case: 'pbr',
                    pbr: material
                }
            });
        },
        getFlat(entity) {
            const mat = theComponent.get(entity);
            return new ReadonlyFlatMaterialAccessor(() => mat);
        },
        getFlatOrNull(entity) {
            const mat = theComponent.getOrNull(entity);
            if (!mat)
                return null;
            return new ReadonlyFlatMaterialAccessor(() => mat);
        },
        getFlatMutable(entity) {
            const getMaterial = () => theComponent.getMutable(entity);
            // Verify component exists immediately (fail-fast)
            getMaterial();
            return new FlatMaterialAccessor(getMaterial);
        },
        getFlatMutableOrNull(entity) {
            const mat = theComponent.getMutableOrNull(entity);
            if (!mat)
                return null;
            return new FlatMaterialAccessor(() => theComponent.getMutable(entity));
        }
    };
}
// Force compile error if assertions fail
const _pbrSyncCheck = true;
const _unlitSyncCheck = true;
