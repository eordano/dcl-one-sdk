import { TriggerArea } from '../generated/index.gen';
export function defineTriggerAreaComponent(engine) {
    const theComponent = TriggerArea(engine);
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
