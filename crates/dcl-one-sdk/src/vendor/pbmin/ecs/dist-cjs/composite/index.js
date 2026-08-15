"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.Composite = exports.getCompositeProvider = exports.setCompositeProvider = exports.getCompositeRootComponent = exports.EntityMappingMode = void 0;
const components_1 = require("./components");
Object.defineProperty(exports, "getCompositeRootComponent", { enumerable: true, get: function () { return components_1.getCompositeRootComponent; } });
const instance_1 = require("./instance");
Object.defineProperty(exports, "EntityMappingMode", { enumerable: true, get: function () { return instance_1.EntityMappingMode; } });
const path_1 = require("./path");
const composite_gen_1 = require("./proto/gen/composite.gen");
var provider_registry_1 = require("./provider-registry");
Object.defineProperty(exports, "setCompositeProvider", { enumerable: true, get: function () { return provider_registry_1.setCompositeProvider; } });
Object.defineProperty(exports, "getCompositeProvider", { enumerable: true, get: function () { return provider_registry_1.getCompositeProvider; } });
/**
 * @public
 * @deprecated composite is not being supported so far, please do not use this feature
 */
var Composite;
(function (Composite) {
    /** @public */
    function fromJson(object) {
        return composite_gen_1.CompositeDefinition.fromJSON(object);
    }
    Composite.fromJson = fromJson;
    /** @public */
    function fromBinary(buffer) {
        return composite_gen_1.CompositeDefinition.decode(buffer);
    }
    Composite.fromBinary = fromBinary;
    /** @public */
    function toJson(composite) {
        return composite_gen_1.CompositeDefinition.toJSON(composite);
    }
    Composite.toJson = toJson;
    /** @public */
    function toBinary(composite) {
        return composite_gen_1.CompositeDefinition.encode(composite).finish();
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
        return (0, instance_1.instanceComposite)(engine, compositeData, compositeProvider, options);
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
        return (0, path_1.resolveComposite)(src, cwd);
    }
    Composite.resolveAndNormalizePath = resolveAndNormalizePath;
})(Composite = exports.Composite || (exports.Composite = {}));
