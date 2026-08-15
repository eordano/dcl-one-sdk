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
exports.defineTransformComponent = exports.TransformSchema = exports.TRANSFORM_LENGTH = exports.COMPONENT_ID = void 0;
// Use import * to safely handle circular dependency (tree.ts → components → Transform.ts → tree.ts).
// With import *, the namespace object's properties are resolved at access time (live bindings in ESM,
// getters via __importStar in CJS), so by the time methods are called all exports are available.
const treeHelpers = __importStar(require("../../runtime/helpers/tree"));
/**
 * @internal
 */
exports.COMPONENT_ID = 1;
/** @internal */
exports.TRANSFORM_LENGTH = 44;
/** @internal */
exports.TransformSchema = {
    serialize(value, builder) {
        const ptr = builder.incrementWriteOffset(exports.TRANSFORM_LENGTH);
        builder.setFloat32(ptr, value.position.x);
        builder.setFloat32(ptr + 4, value.position.y);
        builder.setFloat32(ptr + 8, value.position.z);
        builder.setFloat32(ptr + 12, value.rotation.x);
        builder.setFloat32(ptr + 16, value.rotation.y);
        builder.setFloat32(ptr + 20, value.rotation.z);
        builder.setFloat32(ptr + 24, value.rotation.w);
        builder.setFloat32(ptr + 28, value.scale.x);
        builder.setFloat32(ptr + 32, value.scale.y);
        builder.setFloat32(ptr + 36, value.scale.z);
        builder.setUint32(ptr + 40, value.parent || 0);
    },
    deserialize(reader) {
        const ptr = reader.incrementReadOffset(exports.TRANSFORM_LENGTH);
        return {
            position: {
                x: reader.getFloat32(ptr),
                y: reader.getFloat32(ptr + 4),
                z: reader.getFloat32(ptr + 8)
            },
            rotation: {
                x: reader.getFloat32(ptr + 12),
                y: reader.getFloat32(ptr + 16),
                z: reader.getFloat32(ptr + 20),
                w: reader.getFloat32(ptr + 24)
            },
            scale: {
                x: reader.getFloat32(ptr + 28),
                y: reader.getFloat32(ptr + 32),
                z: reader.getFloat32(ptr + 36)
            },
            parent: reader.getUint32(ptr + 40)
        };
    },
    create() {
        return {
            position: { x: 0, y: 0, z: 0 },
            scale: { x: 1, y: 1, z: 1 },
            rotation: { x: 0, y: 0, z: 0, w: 1 },
            parent: 0
        };
    },
    extend(value) {
        return {
            position: { x: 0, y: 0, z: 0 },
            scale: { x: 1, y: 1, z: 1 },
            rotation: { x: 0, y: 0, z: 0, w: 1 },
            parent: 0,
            ...value
        };
    },
    jsonSchema: {
        type: 'object',
        properties: {
            position: {
                type: 'object',
                properties: {
                    x: { type: 'number' },
                    y: { type: 'number' },
                    z: { type: 'number' }
                }
            },
            scale: {
                type: 'object',
                properties: {
                    x: { type: 'number' },
                    y: { type: 'number' },
                    z: { type: 'number' }
                }
            },
            rotation: {
                type: 'object',
                properties: {
                    x: { type: 'number' },
                    y: { type: 'number' },
                    z: { type: 'number' },
                    w: { type: 'number' }
                }
            },
            parent: { type: 'integer' }
        },
        serializationType: 'transform'
    }
};
function defineTransformComponent(engine) {
    const transformDef = engine.defineComponentFromSchema('core::Transform', exports.TransformSchema);
    return {
        ...transformDef,
        create(entity, val) {
            return transformDef.create(entity, exports.TransformSchema.extend(val));
        },
        createOrReplace(entity, val) {
            return transformDef.createOrReplace(entity, exports.TransformSchema.extend(val));
        },
        localToWorldDirection(entity, localDirection) {
            const worldRotation = treeHelpers.getWorldRotation(engine, entity);
            return treeHelpers.rotateVectorByQuaternion(localDirection, worldRotation);
        }
    };
}
exports.defineTransformComponent = defineTransformComponent;
