import { getCompositeRootComponent } from './components';
import { EntityMappingMode, instanceComposite } from './instance';
import { resolveComposite } from './path';
import { CompositeDefinition } from './proto/gen/composite.gen';
export { EntityMappingMode };
export { getCompositeRootComponent };
export { setCompositeProvider, getCompositeProvider } from './provider-registry';
/**
 * @public
 * @deprecated composite is not being supported so far, please do not use this feature
 */
export var Composite;
(function (Composite) {
    /** @public */
    function fromJson(object) {
        return CompositeDefinition.fromJSON(object);
    }
    Composite.fromJson = fromJson;
    /** @public */
    function fromBinary(buffer) {
        return CompositeDefinition.decode(buffer);
    }
    Composite.fromBinary = fromBinary;
    /** @public */
    function toJson(composite) {
        return CompositeDefinition.toJSON(composite);
    }
    Composite.toJson = toJson;
    /** @public */
    function toBinary(composite) {
        return CompositeDefinition.encode(composite).finish();
    }
    Composite.toBinary = toBinary;
    /**
     * Instance a composite and return its root entity.
     * @param engine - the engine that will own the new entities
     * @param compositeData - the composite resource to instance
     * @param compositeProvider - provider used to resolve nested composite references
     * @param options - instancing options (`rootEntity`, `entityMapping`, `alreadyRequestedSrc`)
     * @returns the root entity of the instanced composite
     *
     * @public
     */
    /*#__PURE__*/ function instance(engine, compositeData, compositeProvider, options = {}) {
        return instanceComposite(engine, compositeData, compositeProvider, options);
    }
    Composite.instance = instance;
    /**
     * Resolve and normalize a composite path
     * @param src - the source path
     * @param cwd - the directory from the resolve should start to resolve
     *
     * @returns the absolute resolved path without slash at the beginning
     * @public
     */
    /*#__PURE__*/ function resolveAndNormalizePath(src, cwd = '/') {
        return resolveComposite(src, cwd);
    }
    Composite.resolveAndNormalizePath = resolveAndNormalizePath;
})(Composite || (Composite = {}));
