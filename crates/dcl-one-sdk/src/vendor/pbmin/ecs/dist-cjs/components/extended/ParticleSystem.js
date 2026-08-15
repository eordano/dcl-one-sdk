"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.defineParticleSystemComponent = exports.ParticleSystemPlaybackState = exports.ParticleSystemBlendMode = void 0;
const index_gen_1 = require("../generated/index.gen");
Object.defineProperty(exports, "ParticleSystemBlendMode", { enumerable: true, get: function () { return index_gen_1.PBParticleSystem_BlendMode; } });
Object.defineProperty(exports, "ParticleSystemPlaybackState", { enumerable: true, get: function () { return index_gen_1.PBParticleSystem_PlaybackState; } });
const ParticleSystemShapeHelper = {
    Point(point = {}) {
        return { $case: 'point', point };
    },
    Sphere(sphere = {}) {
        return { $case: 'sphere', sphere };
    },
    Cone(cone = {}) {
        return { $case: 'cone', cone };
    },
    Box(box = {}) {
        return { $case: 'box', box };
    }
};
function defineParticleSystemComponent(engine) {
    const theComponent = (0, index_gen_1.ParticleSystem)(engine);
    return {
        ...theComponent,
        Shape: ParticleSystemShapeHelper
    };
}
exports.defineParticleSystemComponent = defineParticleSystemComponent;
