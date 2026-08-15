"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.defineTriggerAreaComponent = void 0;
const index_gen_1 = require("../generated/index.gen");
function defineTriggerAreaComponent(engine) {
    const theComponent = (0, index_gen_1.TriggerArea)(engine);
    function getCollisionMask(layers) {
        if (Array.isArray(layers)) {
            return layers.map((item) => item).reduce((prev, item) => prev | item, 0);
        }
        else if (layers) {
            return layers;
        }
    }
    return {
        ...theComponent,
        setBox(entity, collisionMask) {
            theComponent.createOrReplace(entity, {
                mesh: 0 /* TriggerAreaMeshType.TAMT_BOX */,
                collisionMask: getCollisionMask(collisionMask)
            });
        },
        setSphere(entity, collisionMask) {
            theComponent.createOrReplace(entity, {
                mesh: 1 /* TriggerAreaMeshType.TAMT_SPHERE */,
                collisionMask: getCollisionMask(collisionMask)
            });
        }
    };
}
exports.defineTriggerAreaComponent = defineTriggerAreaComponent;
