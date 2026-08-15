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
Object.defineProperty(exports, "__esModule", { value: true });
exports.createAssetLoadLoadingStateSystem = void 0;
const components = __importStar(require("../components"));
const entity_1 = require("../engine/entity");
/**
 * @internal
 */
function createAssetLoadLoadingStateSystem(engine) {
    const assetLoadLoadingStateComponent = components.AssetLoadLoadingState(engine);
    const entitiesCallbackAssetLoadLoadingStateMap = new Map();
    function registerAssetLoadLoadingStateEntity(entity, callback) {
        entitiesCallbackAssetLoadLoadingStateMap.set(entity, { callback: callback, lastLoadingStateLength: 0 });
    }
    function removeAssetLoadLoadingStateEntity(entity) {
        entitiesCallbackAssetLoadLoadingStateMap.delete(entity);
    }
    // @internal
    engine.addSystem(function EventSystem() {
        const garbageEntries = [];
        for (const [entity, data] of entitiesCallbackAssetLoadLoadingStateMap) {
            if (engine.getEntityState(entity) === entity_1.EntityState.Removed) {
                garbageEntries.push(entity);
                continue;
            }
            const loadingState = assetLoadLoadingStateComponent.get(entity);
            if (loadingState.size === 0 || loadingState.size === data.lastLoadingStateLength)
                continue;
            // Get last added values (can be multiple per tick, just not for the same asset)
            const lastValues = Array.from(loadingState.values()).slice(data.lastLoadingStateLength);
            lastValues.forEach((value) => {
                data.callback(value);
            });
            entitiesCallbackAssetLoadLoadingStateMap.set(entity, {
                callback: data.callback,
                lastLoadingStateLength: loadingState.size
            });
        }
        // Clean up garbage entries
        garbageEntries.forEach((garbageEntity) => entitiesCallbackAssetLoadLoadingStateMap.delete(garbageEntity));
    });
    return {
        removeAssetLoadLoadingStateEntity(entity) {
            removeAssetLoadLoadingStateEntity(entity);
        },
        registerAssetLoadLoadingStateEntity(entity, callback) {
            registerAssetLoadLoadingStateEntity(entity, callback);
        }
    };
}
exports.createAssetLoadLoadingStateSystem = createAssetLoadLoadingStateSystem;
