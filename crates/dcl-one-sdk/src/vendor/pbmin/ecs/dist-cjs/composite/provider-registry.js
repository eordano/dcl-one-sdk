"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.getCompositeProvider = exports.setCompositeProvider = void 0;
const schemas_1 = require("../schemas");
const components_1 = require("./components");
let currentProvider = null;
/**
 * Register the composite provider used to resolve composite files. Also registers
 * any `provider.schemas` on the engine pre-seal so composites that reference those
 * components can be instanced without further setup.
 * @public
 */
function setCompositeProvider(engine, provider) {
    currentProvider = provider;
    // Define composite::root pre-seal. setCompositeProvider runs at module-load,
    // so this guarantees the component exists before the engine seals — composites
    // instanced at runtime (post-seal) would otherwise trip the seal when
    // instanceComposite looks it up via getCompositeRootComponent.
    (0, components_1.getCompositeRootComponent)(engine);
    if (!provider.schemas)
        return;
    for (const { name, jsonSchema } of provider.schemas) {
        if (engine.getComponentOrNull(name))
            continue;
        engine.defineComponentFromSchema(name, schemas_1.Schemas.fromJson(jsonSchema));
    }
}
exports.setCompositeProvider = setCompositeProvider;
/**
 * Get the composite provider registered via setCompositeProvider. Returns null if
 * no provider has been set.
 * @public
 */
function getCompositeProvider() {
    return currentProvider;
}
exports.getCompositeProvider = getCompositeProvider;
